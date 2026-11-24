//! PCI highlight information.

use super::{MuxError, invalid};

/// One highlight button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HliButton {
    /// Left edge, inclusive.
    pub x1: u16,
    /// Top edge, inclusive.
    pub y1: u16,
    /// Right edge, inclusive.
    pub x2: u16,
    /// Bottom edge, inclusive.
    pub y2: u16,
    /// Highlight colour set, 1..=3.
    pub color_set: u8,
    /// True when selecting the button triggers an action.
    pub auto_action: bool,
    /// Up adjacent button, 1-based.
    pub up: u8,
    /// Down adjacent button, 1-based.
    pub down: u8,
    /// Left adjacent button, 1-based.
    pub left: u8,
    /// Right adjacent button, 1-based.
    pub right: u8,
    /// VM command executed when the button is selected.
    pub command: [u8; 8],
}

/// Highlight information for one VOBU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hli {
    /// Highlight start presentation time, 90 kHz.
    pub start_ptm: u32,
    /// Buttons in order.
    pub buttons: Vec<HliButton>,
    /// The same buttons where a 4:3 television shows a 16:9 menu letterboxed: a second button group (display
    /// type letterbox; the first is then wide), at most 18 a group, as Encore writes a 16:9 menu. Empty for one
    /// group.
    pub letterbox: Vec<HliButton>,
    /// Default button, 1-based, 0 = none.
    pub default_button: u8,
    /// Button number offset (Encore's menu Offset, BTN_OFN as Encore writes it): the first button is number
    /// `button_offset + 1` on the remote's number keys. 0 for most menus.
    pub button_offset: u8,
    /// Three SL_COLI entries.
    pub color_sets: [[u8; 8]; 3],
}

/// Offset of HLI inside PCI data.
pub const PCI_HLI_OFFSET: usize = 0x60;

const STATUS_OFFSET: usize = PCI_HLI_OFFSET;
const START_PTM_OFFSET: usize = 0x62;
const END_PTM_OFFSET: usize = 0x66;
const BUTTON_SEL_END_OFFSET: usize = 0x6A;
const BUTTON_MODE_OFFSET: usize = 0x6E;
const STARTING_BUTTON_OFFSET: usize = 0x70;
const NUM_BUTTONS_OFFSET: usize = 0x71;
const NUM_SELECTABLE_OFFSET: usize = 0x72;
const FORCED_SELECT_OFFSET: usize = 0x74;
const FORCED_ACTION_OFFSET: usize = 0x75;
const SL_COLI_OFFSET: usize = 0x76;
const BTN_IT_OFFSET: usize = 0x8E;

const STATUS_VALUE: u16 = 0x0001;
const END_PTM_VALUE: u32 = 0xFFFF_FFFF;
/// BTN_MD: the number of button groups (bits 13..12) and each group's display type (bits 10..8, 6..4, 2..0;
/// 1 = normal / wide, 2 = letterbox). One group of type 1.
const BUTTON_MODE_ONE_GROUP: u16 = 0x1100;
/// Two groups, wide and letterbox (Encore's 16:9 menu).
const BUTTON_MODE_WIDE_LETTERBOX: u16 = 0x2120;
const SL_COLI_LEN: usize = 8;
const BTN_IT_LEN: usize = 18;
const BTN_IT_COUNT: usize = 36;
/// The entries of each group when there are two: the second group starts at entry 19.
const TWO_GROUP_LEN: usize = 18;
const HLI_TOTAL_LEN: usize = BTN_IT_OFFSET + BTN_IT_COUNT * BTN_IT_LEN;
const MAX_X: u16 = 720;
const MAX_Y: u16 = 576;

/// Write HLI into the 979-byte PCI data area.
pub fn write_hli(hli: &Hli, pci: &mut [u8]) -> Result<(), MuxError> {
    validate_hli(hli)?;
    if pci.len() < HLI_TOTAL_LEN {
        return Err(invalid("PCI buffer too small"));
    }

    pci.get_mut(PCI_HLI_OFFSET..HLI_TOTAL_LEN)
        .ok_or_else(|| invalid("PCI HLI area out of range"))?
        .fill(0);

    write_hli_header(hli, pci)?;
    write_sl_coli(hli, pci)?;
    write_btn_it(hli, pci)
}

/// Read HLI from the 979-byte PCI data area.
pub fn read_hli(pci: &[u8]) -> Result<Option<Hli>, MuxError> {
    let status_end = STATUS_OFFSET
        .checked_add(2)
        .ok_or_else(|| invalid("status offset overflow"))?;
    if pci.len() < status_end {
        return Err(invalid("PCI buffer too small"));
    }

    let status = get_u16_be(pci, STATUS_OFFSET)?;
    if status == 0 {
        return Ok(None);
    }
    if pci.len() < HLI_TOTAL_LEN {
        return Err(invalid("PCI buffer too small"));
    }

    let header = match read_hli_header(pci)? {
        Some(header) => header,
        None => return Ok(None),
    };
    let color_sets = read_color_sets(pci)?;
    let buttons = read_buttons(pci, 0, header.button_count)?;
    let letterbox = if header.letterbox {
        read_buttons(pci, TWO_GROUP_LEN, header.button_count)?
    } else {
        Vec::new()
    };

    Ok(Some(Hli {
        start_ptm: header.start_ptm,
        buttons,
        letterbox,
        default_button: header.default_button,
        button_offset: header.button_offset,
        color_sets,
    }))
}

struct HliHeader {
    start_ptm: u32,
    default_button: u8,
    button_offset: u8,
    button_count: usize,
    letterbox: bool,
}

fn validate_hli(hli: &Hli) -> Result<(), MuxError> {
    let count = hli.buttons.len();
    let most = if hli.letterbox.is_empty() {
        BTN_IT_COUNT
    } else {
        TWO_GROUP_LEN
    };
    if count == 0 || count > most {
        return Err(invalid("button count out of range"));
    }
    if !hli.letterbox.is_empty() && hli.letterbox.len() != count {
        return Err(invalid("letterbox button count differs"));
    }
    if usize::from(hli.button_offset).saturating_add(count) > BTN_IT_COUNT {
        return Err(invalid("button number offset too large"));
    }
    if hli.default_button != 0 && usize::from(hli.default_button) > count {
        return Err(invalid("default button out of range"));
    }

    for button in hli.buttons.iter().chain(&hli.letterbox) {
        validate_button(button, count)?;
    }

    Ok(())
}

fn validate_button(button: &HliButton, count: usize) -> Result<(), MuxError> {
    if button.x2 >= MAX_X || button.y2 >= MAX_Y {
        return Err(invalid("button rectangle out of range"));
    }
    if button.x1 > button.x2 || button.y1 > button.y2 {
        return Err(invalid("inverted button rectangle"));
    }
    if !(1..=3).contains(&button.color_set) {
        return Err(invalid("button colour set out of range"));
    }

    validate_adjacency(button.up, count)?;
    validate_adjacency(button.down, count)?;
    validate_adjacency(button.left, count)?;
    validate_adjacency(button.right, count)?;

    Ok(())
}

fn validate_adjacency(value: u8, count: usize) -> Result<(), MuxError> {
    if value == 0 || usize::from(value) > count {
        return Err(invalid("button adjacency out of range"));
    }
    Ok(())
}

fn write_hli_header(hli: &Hli, pci: &mut [u8]) -> Result<(), MuxError> {
    let count = u8::try_from(hli.buttons.len()).map_err(|_| invalid("too many buttons for HLI"))?;

    put_u16_be(pci, STATUS_OFFSET, STATUS_VALUE)?;
    put_u32_be(pci, START_PTM_OFFSET, hli.start_ptm)?;
    put_u32_be(pci, END_PTM_OFFSET, END_PTM_VALUE)?;
    put_u32_be(pci, BUTTON_SEL_END_OFFSET, END_PTM_VALUE)?;
    let mode = if hli.letterbox.is_empty() {
        BUTTON_MODE_ONE_GROUP
    } else {
        BUTTON_MODE_WIDE_LETTERBOX
    };
    put_u16_be(pci, BUTTON_MODE_OFFSET, mode)?;
    // Encore writes the menu's Offset as it is (0 for most menus).
    put_u8(pci, STARTING_BUTTON_OFFSET, hli.button_offset)?;
    put_u8(pci, NUM_BUTTONS_OFFSET, count)?;
    put_u8(pci, NUM_SELECTABLE_OFFSET, count)?;
    put_u8(pci, FORCED_SELECT_OFFSET, hli.default_button)?;
    put_u8(pci, FORCED_ACTION_OFFSET, 0)
}

fn read_hli_header(pci: &[u8]) -> Result<Option<HliHeader>, MuxError> {
    let status = get_u16_be(pci, STATUS_OFFSET)?;
    if status == 0 {
        return Ok(None);
    }
    if status != STATUS_VALUE {
        return Err(invalid("bad HLI status"));
    }

    let letterbox = match get_u16_be(pci, BUTTON_MODE_OFFSET)? {
        BUTTON_MODE_ONE_GROUP => false,
        BUTTON_MODE_WIDE_LETTERBOX => true,
        _ => return Err(invalid("bad HLI button mode")),
    };

    let button_offset = get_u8(pci, STARTING_BUTTON_OFFSET)?;
    if usize::from(button_offset) >= BTN_IT_COUNT {
        return Err(invalid("bad HLI button offset"));
    }

    let most = if letterbox {
        TWO_GROUP_LEN
    } else {
        BTN_IT_COUNT
    };
    let button_count = usize::from(get_u8(pci, NUM_BUTTONS_OFFSET)?);
    if button_count == 0 || button_count > most {
        return Err(invalid("bad HLI button count"));
    }

    let selectable = usize::from(get_u8(pci, NUM_SELECTABLE_OFFSET)?);
    if selectable != button_count {
        return Err(invalid("bad HLI selectable button count"));
    }

    let default_button = get_u8(pci, FORCED_SELECT_OFFSET)?;
    if default_button != 0 && usize::from(default_button) > button_count {
        return Err(invalid("bad HLI default button"));
    }

    let forced_action = get_u8(pci, FORCED_ACTION_OFFSET)?;
    if forced_action != 0 {
        return Err(invalid("bad HLI forced action button"));
    }

    let start_ptm = get_u32_be(pci, START_PTM_OFFSET)?;
    let end_ptm = get_u32_be(pci, END_PTM_OFFSET)?;
    if end_ptm != END_PTM_VALUE {
        return Err(invalid("bad HLI end PTM"));
    }

    let button_sel_end = get_u32_be(pci, BUTTON_SEL_END_OFFSET)?;
    if button_sel_end != END_PTM_VALUE {
        return Err(invalid("bad HLI button selection end PTM"));
    }

    Ok(Some(HliHeader {
        start_ptm,
        default_button,
        button_offset,
        button_count,
        letterbox,
    }))
}

fn write_sl_coli(hli: &Hli, pci: &mut [u8]) -> Result<(), MuxError> {
    for (index, src) in hli.color_sets.iter().enumerate() {
        let offset = sl_coli_offset(index)?;
        put_slice(pci, offset, src)?;
    }
    Ok(())
}

fn read_color_sets(pci: &[u8]) -> Result<[[u8; 8]; 3], MuxError> {
    let mut color_sets = [[0u8; 8]; 3];
    for (index, dst) in color_sets.iter_mut().enumerate() {
        let offset = sl_coli_offset(index)?;
        let src = get_slice(pci, offset, SL_COLI_LEN)?;
        dst.copy_from_slice(src);
    }
    Ok(color_sets)
}

fn write_btn_it(hli: &Hli, pci: &mut [u8]) -> Result<(), MuxError> {
    let groups = [(0, &hli.buttons), (TWO_GROUP_LEN, &hli.letterbox)];
    for (first, buttons) in groups {
        for (index, button) in buttons.iter().enumerate() {
            let entry = first
                .checked_add(index)
                .ok_or_else(|| invalid("button index overflow"))?;
            write_btn_entry(button, pci, entry_offset(entry)?)?;
        }
    }
    Ok(())
}

/// The `count` buttons of the group whose entries start at `first` (0, or 18 for a second group).
fn read_buttons(pci: &[u8], first: usize, count: usize) -> Result<Vec<HliButton>, MuxError> {
    let mut buttons = Vec::with_capacity(count);
    for index in 0..count {
        let entry = first
            .checked_add(index)
            .ok_or_else(|| invalid("button index overflow"))?;
        buttons.push(read_btn_entry(pci, entry_offset(entry)?, count)?);
    }
    Ok(buttons)
}

fn write_btn_entry(button: &HliButton, pci: &mut [u8], base: usize) -> Result<(), MuxError> {
    let x1_high = (button.x1 >> 4) & 0x3F;
    let x1_low = (button.x1 & 0xF) << 4;
    let x2_high = (button.x2 >> 8) & 0xF;
    let x2_low = button.x2 & 0xFF;

    let y1_high = (button.y1 >> 4) & 0x3F;
    let y1_low = (button.y1 & 0xF) << 4;
    let y2_high = (button.y2 >> 8) & 0xF;
    let y2_low = button.y2 & 0xFF;

    let auto_action = if button.auto_action { 1u8 << 6 } else { 0 };

    let byte0 = (button.color_set << 6)
        | u8::try_from(x1_high).map_err(|_| invalid("x1 high out of range"))?;
    let byte1 = u8::try_from(x1_low | x2_high).map_err(|_| invalid("x1/x2 out of range"))?;
    let byte2 = u8::try_from(x2_low).map_err(|_| invalid("x2 low out of range"))?;
    let byte3 = auto_action | u8::try_from(y1_high).map_err(|_| invalid("y1 high out of range"))?;
    let byte4 = u8::try_from(y1_low | y2_high).map_err(|_| invalid("y1/y2 out of range"))?;
    let byte5 = u8::try_from(y2_low).map_err(|_| invalid("y2 low out of range"))?;

    put_u8_at(pci, base, 0, byte0)?;
    put_u8_at(pci, base, 1, byte1)?;
    put_u8_at(pci, base, 2, byte2)?;
    put_u8_at(pci, base, 3, byte3)?;
    put_u8_at(pci, base, 4, byte4)?;
    put_u8_at(pci, base, 5, byte5)?;
    put_u8_at(pci, base, 6, button.up)?;
    put_u8_at(pci, base, 7, button.down)?;
    put_u8_at(pci, base, 8, button.left)?;
    put_u8_at(pci, base, 9, button.right)?;
    put_slice_at(pci, base, 10, &button.command)
}

fn read_btn_entry(pci: &[u8], base: usize, count: usize) -> Result<HliButton, MuxError> {
    let byte0 = get_u8_at(pci, base, 0)?;
    let byte1 = get_u8_at(pci, base, 1)?;
    let byte2 = get_u8_at(pci, base, 2)?;
    let byte3 = get_u8_at(pci, base, 3)?;
    let byte4 = get_u8_at(pci, base, 4)?;
    let byte5 = get_u8_at(pci, base, 5)?;
    let up = get_u8_at(pci, base, 6)?;
    let down = get_u8_at(pci, base, 7)?;
    let left = get_u8_at(pci, base, 8)?;
    let right = get_u8_at(pci, base, 9)?;

    let mut command = [0u8; 8];
    let cmd = get_slice_at(pci, base, 10, 8)?;
    command.copy_from_slice(cmd);

    let button = HliButton {
        x1: (u16::from(byte0 & 0x3F) << 4) | u16::from(byte1 >> 4),
        y1: (u16::from(byte3 & 0x3F) << 4) | u16::from(byte4 >> 4),
        x2: (u16::from(byte1 & 0x0F) << 8) | u16::from(byte2),
        y2: (u16::from(byte4 & 0x0F) << 8) | u16::from(byte5),
        color_set: byte0 >> 6,
        auto_action: (byte3 & 0x40) != 0,
        up,
        down,
        left,
        right,
        command,
    };

    validate_button(&button, count)?;
    Ok(button)
}

fn sl_coli_offset(index: usize) -> Result<usize, MuxError> {
    let delta = index
        .checked_mul(SL_COLI_LEN)
        .ok_or_else(|| invalid("SL_COLI index overflow"))?;
    SL_COLI_OFFSET
        .checked_add(delta)
        .ok_or_else(|| invalid("SL_COLI offset overflow"))
}

fn entry_offset(index: usize) -> Result<usize, MuxError> {
    let delta = index
        .checked_mul(BTN_IT_LEN)
        .ok_or_else(|| invalid("button index overflow"))?;
    BTN_IT_OFFSET
        .checked_add(delta)
        .ok_or_else(|| invalid("BTN_IT offset overflow"))
}

fn put_u8(buf: &mut [u8], offset: usize, value: u8) -> Result<(), MuxError> {
    let slot = buf
        .get_mut(offset)
        .ok_or_else(|| invalid("HLI write out of range"))?;
    *slot = value;
    Ok(())
}

fn put_u8_at(buf: &mut [u8], base: usize, delta: usize, value: u8) -> Result<(), MuxError> {
    let offset = base
        .checked_add(delta)
        .ok_or_else(|| invalid("HLI offset overflow"))?;
    put_u8(buf, offset, value)
}

fn put_u16_be(buf: &mut [u8], offset: usize, value: u16) -> Result<(), MuxError> {
    put_slice(buf, offset, &value.to_be_bytes())
}

fn put_u32_be(buf: &mut [u8], offset: usize, value: u32) -> Result<(), MuxError> {
    put_slice(buf, offset, &value.to_be_bytes())
}

fn put_slice(buf: &mut [u8], offset: usize, src: &[u8]) -> Result<(), MuxError> {
    if src.is_empty() {
        return Ok(());
    }
    let end = offset
        .checked_add(src.len())
        .ok_or_else(|| invalid("HLI offset overflow"))?;
    let dst = buf
        .get_mut(offset..end)
        .ok_or_else(|| invalid("HLI write out of range"))?;
    dst.copy_from_slice(src);
    Ok(())
}

fn put_slice_at(buf: &mut [u8], base: usize, delta: usize, src: &[u8]) -> Result<(), MuxError> {
    let offset = base
        .checked_add(delta)
        .ok_or_else(|| invalid("HLI offset overflow"))?;
    put_slice(buf, offset, src)
}

fn get_u8(buf: &[u8], offset: usize) -> Result<u8, MuxError> {
    buf.get(offset)
        .copied()
        .ok_or_else(|| invalid("HLI read out of range"))
}

fn get_u8_at(buf: &[u8], base: usize, delta: usize) -> Result<u8, MuxError> {
    let offset = base
        .checked_add(delta)
        .ok_or_else(|| invalid("HLI offset overflow"))?;
    get_u8(buf, offset)
}

fn get_u16_be(buf: &[u8], offset: usize) -> Result<u16, MuxError> {
    let bytes: [u8; 2] = get_slice(buf, offset, 2)?
        .try_into()
        .map_err(|_| invalid("bad u16"))?;
    Ok(u16::from_be_bytes(bytes))
}

fn get_u32_be(buf: &[u8], offset: usize) -> Result<u32, MuxError> {
    let bytes: [u8; 4] = get_slice(buf, offset, 4)?
        .try_into()
        .map_err(|_| invalid("bad u32"))?;
    Ok(u32::from_be_bytes(bytes))
}

fn get_slice(buf: &[u8], offset: usize, len: usize) -> Result<&[u8], MuxError> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| invalid("HLI offset overflow"))?;
    buf.get(offset..end)
        .ok_or_else(|| invalid("HLI read out of range"))
}

fn get_slice_at(buf: &[u8], base: usize, delta: usize, len: usize) -> Result<&[u8], MuxError> {
    let offset = base
        .checked_add(delta)
        .ok_or_else(|| invalid("HLI offset overflow"))?;
    get_slice(buf, offset, len)
}

#[cfg(test)]
mod tests {
    use super::{Hli, HliButton, MuxError, read_hli, write_hli};

    fn button_with_adj(up: u8, down: u8, left: u8, right: u8) -> HliButton {
        HliButton {
            x1: 10,
            y1: 10,
            x2: 100,
            y2: 100,
            color_set: 1,
            auto_action: false,
            up,
            down,
            left,
            right,
            command: [0u8; 8],
        }
    }

    #[test]
    fn round_trip_three_buttons() -> Result<(), MuxError> {
        let hli = Hli {
            start_ptm: 12345,
            buttons: vec![
                button_with_adj(2, 3, 1, 1),
                button_with_adj(3, 1, 2, 2),
                button_with_adj(1, 2, 3, 3),
            ],
            letterbox: Vec::new(),
            default_button: 2,
            button_offset: 0,
            color_sets: [
                [1, 2, 3, 4, 5, 6, 7, 8],
                [9, 10, 11, 12, 13, 14, 15, 16],
                [17, 18, 19, 20, 21, 22, 23, 24],
            ],
        };

        let mut pci = vec![0u8; 979];
        write_hli(&hli, &mut pci)?;
        let got = read_hli(&pci)?.ok_or_else(|| MuxError::Invalid("missing HLI".to_string()))?;

        if got != hli {
            return Err(MuxError::Invalid("HLI round trip mismatch".to_string()));
        }
        Ok(())
    }

    #[test]
    fn btn_it_byte_exact() -> Result<(), MuxError> {
        let command = [0x20, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05];
        let button = HliButton {
            x1: 160,
            y1: 120,
            x2: 559,
            y2: 167,
            color_set: 1,
            auto_action: false,
            up: 3,
            down: 2,
            left: 1,
            right: 1,
            command,
        };
        let hli = Hli {
            start_ptm: 0,
            // Buttons 2 and 3 exist so the adjacency numbers of button 1 are valid.
            buttons: vec![button.clone(), button.clone(), button],
            letterbox: Vec::new(),
            default_button: 1,
            button_offset: 0,
            color_sets: [[0u8; 8]; 3],
        };

        let mut pci = vec![0u8; 979];
        write_hli(&hli, &mut pci)?;

        let expected = [0x4A, 0x02, 0x2F, 0x07, 0x80, 0xA7, 3, 2, 1, 1];
        let got = pci
            .get(0x8E..0x98)
            .ok_or_else(|| MuxError::Invalid("missing BTN_IT bytes".to_string()))?;
        if got != &expected[..] {
            return Err(MuxError::Invalid("BTN_IT byte mismatch".to_string()));
        }

        let got_cmd = pci
            .get(0x98..0xA0)
            .ok_or_else(|| MuxError::Invalid("missing BTN_IT command".to_string()))?;
        if got_cmd != &command[..] {
            return Err(MuxError::Invalid("BTN_IT command mismatch".to_string()));
        }

        Ok(())
    }

    /// Encore CS6's 16:9 menu (2026-11-23, the Gray menu set to 16:9, 18 buttons, Offset 5): BTN_MD `0x2120` (two
    /// groups, wide then letterbox), BTN_OFN 5, BTN_NS 18, the letterbox group from entry 19 with the same routes
    /// and commands. A 4:3 menu's Offset 0 is BTN_OFN 0.
    #[test]
    fn a_wide_menu_has_a_letterbox_group_as_encore_writes_it() -> Result<(), MuxError> {
        let wide = HliButton {
            x1: 196,
            y1: 303,
            x2: 262,
            y2: 312,
            ..button_with_adj(2, 2, 2, 2)
        };
        let letterbox = HliButton {
            y1: 287,
            y2: 294,
            ..wide.clone()
        };
        let mut hli = Hli {
            start_ptm: 0,
            buttons: vec![wide.clone(), wide],
            letterbox: vec![letterbox.clone(), letterbox],
            default_button: 0,
            button_offset: 5,
            color_sets: [[0u8; 8]; 3],
        };
        let mut pci = vec![0u8; 979];
        write_hli(&hli, &mut pci)?;
        let header = pci.get(0x6E..0x72).map(<[u8]>::to_vec);
        assert_eq!(header, Some(vec![0x21, 0x20, 5, 2]));
        // Entry 19 (0x8E + 18 x 18): x 196..262, y 287..294.
        let entry = pci.get(0x1D2..0x1D8).map(<[u8]>::to_vec);
        assert_eq!(entry, Some(vec![0x4C, 0x41, 0x06, 0x11, 0xF1, 0x26]));
        assert_eq!(read_hli(&pci)?.as_ref(), Some(&hli));

        hli.letterbox.clear();
        hli.button_offset = 0;
        write_hli(&hli, &mut pci)?;
        assert_eq!(pci.get(0x6E..0x71), Some(&[0x11, 0x00, 0][..]));
        assert_eq!(pci.get(0x1D2..0x1D8), Some(&[0u8; 6][..]));
        assert_eq!(read_hli(&pci)?.as_ref(), Some(&hli));

        // A second group holds 18 buttons at most, as many as the first.
        hli.buttons = vec![button_with_adj(1, 1, 1, 1); 19];
        hli.letterbox = hli.buttons.clone();
        assert!(write_hli(&hli, &mut pci).is_err());
        hli.buttons.truncate(3);
        hli.letterbox.truncate(2);
        assert!(write_hli(&hli, &mut pci).is_err());
        Ok(())
    }

    #[test]
    fn too_many_buttons_is_error() -> Result<(), MuxError> {
        let mut buttons = Vec::new();
        for _ in 0..37 {
            buttons.push(button_with_adj(1, 1, 1, 1));
        }
        let hli = Hli {
            start_ptm: 0,
            buttons,
            letterbox: Vec::new(),
            default_button: 1,
            button_offset: 0,
            color_sets: [[0u8; 8]; 3],
        };

        let mut pci = vec![0u8; 979];
        if write_hli(&hli, &mut pci).is_ok() {
            return Err(MuxError::Invalid(
                "expected too many buttons error".to_string(),
            ));
        }
        Ok(())
    }

    #[test]
    fn corrupted_buffers_do_not_panic() -> Result<(), MuxError> {
        let short = [0u8; 10];
        if read_hli(&short).is_ok() {
            return Err(MuxError::Invalid("expected short buffer error".to_string()));
        }

        let mut pci = vec![0u8; 979];
        if let Some(slot) = pci.get_mut(0x60..0x62) {
            slot.copy_from_slice(&2u16.to_be_bytes());
        }
        if read_hli(&pci).is_ok() {
            return Err(MuxError::Invalid("expected bad status error".to_string()));
        }

        let mut pci = vec![0u8; 979];
        if let Some(slot) = pci.get_mut(0x60..0x62) {
            slot.copy_from_slice(&1u16.to_be_bytes());
        }
        if read_hli(&pci).is_ok() {
            return Err(MuxError::Invalid(
                "expected zero button count error".to_string(),
            ));
        }

        let mut short = vec![0u8; 10];
        let hli = Hli {
            start_ptm: 0,
            buttons: vec![button_with_adj(1, 1, 1, 1)],
            letterbox: Vec::new(),
            default_button: 1,
            button_offset: 0,
            color_sets: [[0u8; 8]; 3],
        };
        if write_hli(&hli, &mut short).is_ok() {
            return Err(MuxError::Invalid("expected short write error".to_string()));
        }

        Ok(())
    }
}
