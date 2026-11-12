//! SPU decoder.

use crate::rle::decode_field;
use crate::{
    Bitmap, DecodedSpu, MAX_DCSQS, MAX_HEIGHT, MAX_PIXELS, MAX_SPU_SIZE, MAX_WIDTH, SpuError,
    SpuParams, spu_error,
};

#[derive(Debug, Clone)]
struct DcsqState {
    x1: Option<u16>,
    y1: Option<u16>,
    x2: Option<u16>,
    y2: Option<u16>,
    top: Option<u16>,
    bottom: Option<u16>,
    colors: [u8; 4],
    alpha: [u8; 4],
    forced: bool,
    stop_delay: Option<u16>,
}

impl DcsqState {
    fn new() -> Self {
        Self {
            x1: None,
            y1: None,
            x2: None,
            y2: None,
            top: None,
            bottom: None,
            colors: [0, 0, 0, 0],
            alpha: [0, 0, 0, 0],
            forced: false,
            stop_delay: None,
        }
    }
}

/// Decode an SPU byte string.
pub fn decode_spu(data: &[u8]) -> Result<DecodedSpu, SpuError> {
    let total = parse_header(data)?;
    let first = parse_first_dcsq(data, total)?;

    let mut state = DcsqState::new();
    parse_dcsqs(data, total, first, &mut state)?;

    let pixels = decode_pixels(data, total, first, &state)?;
    let (x, y, x2, y2) = get_area(&state)?;
    // SET_DAREA coordinates are inclusive.
    let width = x2
        .checked_sub(x)
        .and_then(|v| v.checked_add(1))
        .ok_or_else(|| spu_error("bad width"))?;
    let height = y2
        .checked_sub(y)
        .and_then(|v| v.checked_add(1))
        .ok_or_else(|| spu_error("bad height"))?;
    let params = make_params(&state, x, y);
    let bitmap = Bitmap {
        width,
        height,
        pixels,
    };
    Ok(DecodedSpu { bitmap, params })
}

fn parse_header(data: &[u8]) -> Result<usize, SpuError> {
    let total = read_u16(data, data.len(), 0).ok_or_else(|| spu_error("missing header"))?;
    let total = usize::from(total);
    if !(4..=MAX_SPU_SIZE).contains(&total) || total > data.len() {
        return Err(spu_error("bad SPU size"));
    }
    Ok(total)
}

fn parse_first_dcsq(data: &[u8], total: usize) -> Result<usize, SpuError> {
    let first = read_u16(data, total, 2).ok_or_else(|| spu_error("missing first DCSQ offset"))?;
    let first = usize::from(first);
    if first < 4 || first >= total {
        return Err(spu_error("bad first DCSQ offset"));
    }
    Ok(first)
}

fn parse_dcsqs(
    data: &[u8],
    total: usize,
    first: usize,
    state: &mut DcsqState,
) -> Result<(), SpuError> {
    let mut offset = first;
    let mut count = 0usize;

    while count < MAX_DCSQS {
        count = count
            .checked_add(1)
            .ok_or_else(|| spu_error("DCSQ count overflow"))?;
        let (delay, next, cmd_start, limit) = parse_dcsq_header(data, total, offset)?;
        parse_commands(data, limit, cmd_start, delay, state)?;
        let next = usize::from(next);
        if next == offset {
            return Ok(());
        }
        offset = next;
    }
    Err(spu_error("too many DCSQs"))
}

fn parse_dcsq_header(
    data: &[u8],
    total: usize,
    offset: usize,
) -> Result<(u16, u16, usize, usize), SpuError> {
    if offset >= total {
        return Err(spu_error("DCSQ offset out of range"));
    }
    let delay = read_u16(data, total, offset).ok_or_else(|| spu_error("missing delay"))?;
    let next_pos = offset
        .checked_add(2)
        .ok_or_else(|| spu_error("offset overflow"))?;
    let next = read_u16(data, total, next_pos).ok_or_else(|| spu_error("missing next offset"))?;
    let cmd_start = offset
        .checked_add(4)
        .ok_or_else(|| spu_error("offset overflow"))?;
    if cmd_start > total {
        return Err(spu_error("DCSQ header out of range"));
    }

    let limit = if usize::from(next) == offset {
        total
    } else {
        let next_usize = usize::from(next);
        if next_usize <= offset || next_usize < cmd_start || next_usize >= total {
            return Err(spu_error("bad next DCSQ offset"));
        }
        next_usize
    };

    Ok((delay, next, cmd_start, limit))
}

fn parse_commands(
    data: &[u8],
    limit: usize,
    mut pos: usize,
    delay: u16,
    state: &mut DcsqState,
) -> Result<(), SpuError> {
    while pos < limit {
        if data.get(pos) == Some(&0xFF) {
            return Ok(()); // CMD_END
        }
        pos = parse_one_command(data, limit, pos, delay, state)?;
    }
    Err(spu_error("missing CMD_END"))
}

fn parse_one_command(
    data: &[u8],
    limit: usize,
    pos: usize,
    delay: u16,
    state: &mut DcsqState,
) -> Result<usize, SpuError> {
    let cmd = *data.get(pos).ok_or_else(|| spu_error("missing command"))?;
    let next = pos
        .checked_add(1)
        .ok_or_else(|| spu_error("command overflow"))?;
    if next > limit {
        return Err(spu_error("command out of range"));
    }

    match cmd {
        0x00..=0x02 => {
            parse_no_arg(cmd, delay, state)?;
            Ok(next)
        }
        0x03 | 0x04 => parse_color_contr(cmd, data, limit, next, state),
        0x05 => parse_area(data, limit, next, state),
        0x06 => parse_dspxa(data, limit, next, state),
        0x07 => Err(spu_error("CHG_COLCON is not supported")),
        0xFF => Ok(next),
        _ => Err(spu_error("unknown command")),
    }
}

fn parse_no_arg(cmd: u8, delay: u16, state: &mut DcsqState) -> Result<(), SpuError> {
    match cmd {
        0x00 => state.forced = true,
        0x01 => state.forced = false,
        0x02 => state.stop_delay = Some(delay),
        _ => return Err(spu_error("bad no-arg command")),
    }
    Ok(())
}

fn parse_color_contr(
    cmd: u8,
    data: &[u8],
    limit: usize,
    pos: usize,
    state: &mut DcsqState,
) -> Result<usize, SpuError> {
    let val = read_u16(data, limit, pos).ok_or_else(|| spu_error("missing color/contrast args"))?;
    let end = pos
        .checked_add(2)
        .ok_or_else(|| spu_error("color/contrast overflow"))?;

    let e2 = ((val >> 12) & 0xF) as u8;
    let e1 = ((val >> 8) & 0xF) as u8;
    let p = ((val >> 4) & 0xF) as u8;
    let b = (val & 0xF) as u8;
    let values = [b, p, e1, e2];

    if cmd == 0x03 {
        state.colors = values;
    } else {
        state.alpha = values;
    }
    Ok(end)
}

fn parse_area(
    data: &[u8],
    limit: usize,
    pos: usize,
    state: &mut DcsqState,
) -> Result<usize, SpuError> {
    let (x1, x2, y1, y2, end) = read_area(data, limit, pos)?;
    validate_area(x1, x2, y1, y2)?;
    state.x1 = Some(x1);
    state.x2 = Some(x2);
    state.y1 = Some(y1);
    state.y2 = Some(y2);
    Ok(end)
}

fn read_area(
    data: &[u8],
    limit: usize,
    pos: usize,
) -> Result<(u16, u16, u16, u16, usize), SpuError> {
    let end = pos
        .checked_add(6)
        .ok_or_else(|| spu_error("area overflow"))?;
    if end > limit {
        return Err(spu_error("area args out of range"));
    }
    let b = data
        .get(pos..end)
        .ok_or_else(|| spu_error("area args missing"))?;

    let b0 = *b.first().ok_or_else(|| spu_error("area byte"))?;
    let b1 = *b.get(1).ok_or_else(|| spu_error("area byte"))?;
    let b2 = *b.get(2).ok_or_else(|| spu_error("area byte"))?;
    let b3 = *b.get(3).ok_or_else(|| spu_error("area byte"))?;
    let b4 = *b.get(4).ok_or_else(|| spu_error("area byte"))?;
    let b5 = *b.get(5).ok_or_else(|| spu_error("area byte"))?;

    let x1 = (u16::from(b0) << 4) | u16::from(b1 >> 4);
    let x2 = ((u16::from(b1) & 0xF) << 8) | u16::from(b2);
    let y1 = (u16::from(b3) << 4) | u16::from(b4 >> 4);
    let y2 = ((u16::from(b4) & 0xF) << 8) | u16::from(b5);

    Ok((x1, x2, y1, y2, end))
}

fn validate_area(x1: u16, x2: u16, y1: u16, y2: u16) -> Result<(), SpuError> {
    if x1 > x2 || y1 > y2 {
        return Err(spu_error("inverted area"));
    }
    if x2 >= MAX_WIDTH || y2 >= MAX_HEIGHT {
        return Err(spu_error("area out of range"));
    }
    Ok(())
}

fn parse_dspxa(
    data: &[u8],
    limit: usize,
    pos: usize,
    state: &mut DcsqState,
) -> Result<usize, SpuError> {
    let top = read_u16(data, limit, pos).ok_or_else(|| spu_error("missing top offset"))?;
    let bottom_pos = pos
        .checked_add(2)
        .ok_or_else(|| spu_error("DSPXA overflow"))?;
    let bottom =
        read_u16(data, limit, bottom_pos).ok_or_else(|| spu_error("missing bottom offset"))?;
    let end = bottom_pos
        .checked_add(2)
        .ok_or_else(|| spu_error("DSPXA overflow"))?;

    state.top = Some(top);
    state.bottom = Some(bottom);
    Ok(end)
}

fn decode_pixels(
    data: &[u8],
    total: usize,
    first: usize,
    state: &DcsqState,
) -> Result<Vec<u8>, SpuError> {
    let (x1, y1, x2, y2) = get_area(state)?;
    let width = x2
        .checked_sub(x1)
        .and_then(|v| v.checked_add(1))
        .ok_or_else(|| spu_error("bad width"))?;
    let height = y2
        .checked_sub(y1)
        .and_then(|v| v.checked_add(1))
        .ok_or_else(|| spu_error("bad height"))?;
    let len = bitmap_len(width, height)?;

    let mut pixels = vec![0u8; len];
    let width_usize = usize::from(width);
    let height_usize = usize::from(height);
    let top_count = height_usize.div_ceil(2);
    let bottom_count = height_usize / 2;
    let (top, bottom) = validate_offsets(state, first, bottom_count)?;

    decode_field(data, first, top, 0, top_count, width_usize, &mut pixels)?;
    if bottom_count > 0 {
        decode_field(
            data,
            first,
            bottom,
            1,
            bottom_count,
            width_usize,
            &mut pixels,
        )?;
    }

    let _ = total;
    Ok(pixels)
}

fn get_area(state: &DcsqState) -> Result<(u16, u16, u16, u16), SpuError> {
    let x1 = state.x1.ok_or_else(|| spu_error("missing SET_DAREA"))?;
    let x2 = state.x2.ok_or_else(|| spu_error("missing SET_DAREA"))?;
    let y1 = state.y1.ok_or_else(|| spu_error("missing SET_DAREA"))?;
    let y2 = state.y2.ok_or_else(|| spu_error("missing SET_DAREA"))?;
    Ok((x1, y1, x2, y2))
}

fn bitmap_len(width: u16, height: u16) -> Result<usize, SpuError> {
    let len = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| spu_error("pixel overflow"))?;
    if len > MAX_PIXELS {
        return Err(spu_error("bitmap too large"));
    }
    Ok(len)
}

fn validate_offsets(
    state: &DcsqState,
    first: usize,
    bottom_count: usize,
) -> Result<(usize, usize), SpuError> {
    let top = usize::from(state.top.ok_or_else(|| spu_error("missing SET_DSPXA"))?);
    let bottom = usize::from(state.bottom.ok_or_else(|| spu_error("missing SET_DSPXA"))?);

    if top >= first {
        return Err(spu_error("top field out of range"));
    }
    if bottom > first {
        return Err(spu_error("bottom field out of range"));
    }
    if bottom_count > 0 && bottom >= first {
        return Err(spu_error("bottom field out of range"));
    }
    Ok((top, bottom))
}

fn make_params(state: &DcsqState, x: u16, y: u16) -> SpuParams {
    SpuParams {
        x,
        y,
        colors: state.colors,
        alpha: state.alpha,
        forced: state.forced,
        stop_delay: state.stop_delay,
    }
}

fn read_u16(data: &[u8], limit: usize, pos: usize) -> Option<u16> {
    let end = pos.checked_add(2)?;
    if end > limit {
        return None;
    }
    let hi = data.get(pos)?;
    let lo = data.get(pos.checked_add(1)?)?;
    Some((u16::from(*hi) << 8) | u16::from(*lo))
}

#[cfg(test)]
mod tests {
    use crate::{Bitmap, SpuError, SpuParams, decode_spu, encode_spu, spu_error};

    fn valid_spu() -> Result<Vec<u8>, SpuError> {
        let bitmap = Bitmap {
            width: 2,
            height: 2,
            pixels: vec![0, 1, 2, 3],
        };
        let params = SpuParams {
            x: 0,
            y: 0,
            colors: [0, 1, 2, 3],
            alpha: [0, 5, 10, 15],
            forced: false,
            stop_delay: None,
        };
        encode_spu(&bitmap, &params)
    }

    fn u16_at(data: &[u8], pos: usize) -> Result<u16, SpuError> {
        let hi = data.get(pos).ok_or_else(|| spu_error("missing byte"))?;
        let lo = data
            .get(pos.checked_add(1).ok_or_else(|| spu_error("overflow"))?)
            .ok_or_else(|| spu_error("missing byte"))?;
        Ok((u16::from(*hi) << 8) | u16::from(*lo))
    }

    #[test]
    fn truncated_prefixes() -> Result<(), SpuError> {
        let data = valid_spu()?;
        for i in 0..=data.len() {
            if let Some(prefix) = data.get(0..i) {
                if i == data.len() {
                    assert!(decode_spu(prefix).is_ok());
                } else {
                    assert!(decode_spu(prefix).is_err());
                }
            }
        }
        Ok(())
    }

    #[test]
    fn corruptions_do_not_panic() -> Result<(), SpuError> {
        let data = valid_spu()?;
        assert!(!data.is_empty());
        for i in 0..200 {
            let mut corrupted = data.clone();
            if let Some(b) = corrupted.get_mut(i % data.len()) {
                *b ^= 0x5A;
            }
            let _ = decode_spu(&corrupted);
        }
        Ok(())
    }

    #[test]
    fn rejects_unknown_commands() -> Result<(), SpuError> {
        let data = valid_spu()?;
        let first = usize::from(u16_at(&data, 2)?);
        let cmd_pos = first.checked_add(4).ok_or_else(|| spu_error("overflow"))?;

        let mut c1 = data.clone();
        if let Some(b) = c1.get_mut(cmd_pos) {
            *b = 0x07;
        }
        assert!(decode_spu(&c1).is_err());

        let mut c2 = data.clone();
        if let Some(b) = c2.get_mut(cmd_pos) {
            *b = 0x08;
        }
        assert!(decode_spu(&c2).is_err());
        Ok(())
    }
}
