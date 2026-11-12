//! MPEG-2 pack, PES, and padding construction.

use super::{MuxError, invalid};
use crate::sink::SECTOR;

const PACK_HEADER_LEN: usize = 14;
const PES_HEADER_BASE: usize = 9;
const SCR_DEN: i128 = 7;
const PACK_NUM: i128 = 307_200;
const LEAD: i128 = 5_400_000;

/// Compute the next pack SCR and remainder.
pub(crate) fn next_scr(
    prev_scr: i128,
    rem: i128,
    deadline: Option<i64>,
) -> Result<(i128, i128), MuxError> {
    let total = prev_scr
        .checked_mul(SCR_DEN)
        .and_then(|v| v.checked_add(rem))
        .and_then(|v| v.checked_add(PACK_NUM))
        .ok_or_else(|| invalid("SCR overflow"))?;
    let min_scr = total / SCR_DEN;
    let mut new_rem = total % SCR_DEN;
    let mut scr = min_scr;

    if let Some(deadline) = deadline {
        let deadline = i128::from(deadline);
        if deadline > LEAD {
            let target = deadline
                .checked_sub(LEAD)
                .ok_or_else(|| invalid("deadline underflow"))?;
            if target > scr {
                scr = target;
                new_rem = 0;
            }
        }
        if scr > deadline {
            return Err(invalid("mux rate exceeded"));
        }
    }

    Ok((scr, new_rem))
}

/// Encode PTS and optionally DTS into PES optional header bytes.
pub(crate) fn pts_bytes(pts: i64, dts: i64, has_dts: bool) -> Result<Vec<u8>, MuxError> {
    let p = timestamp_to_u64(pts)?;
    let d = timestamp_to_u64(dts)?;

    let mut bits: u128 = 0;
    let mut nbits: u32 = 0;
    append_bits(&mut bits, &mut nbits, if has_dts { 3 } else { 2 }, 4);
    append_timestamp_field(&mut bits, &mut nbits, p);
    if has_dts {
        append_bits(&mut bits, &mut nbits, 1, 4);
        append_timestamp_field(&mut bits, &mut nbits, d);
    }

    let mut out = Vec::with_capacity((nbits / 8) as usize);
    write_bits_vec(bits, nbits, &mut out)?;
    Ok(out)
}

/// Write one complete data pack containing one PES packet and padding/stuffing.
pub(crate) fn write_pes_pack(
    sector: &mut [u8; SECTOR],
    scr: i128,
    stream_id: u8,
    flags2: u8,
    header_data_len: usize,
    optional: &[u8],
    payload: &[u8],
) -> Result<(), MuxError> {
    if optional.len() != header_data_len {
        return Err(invalid("PES header length mismatch"));
    }

    let mut hdr_len = header_data_len;
    let mut opt = optional.to_vec();
    let pay_len = payload.len();

    let used = PACK_HEADER_LEN
        .checked_add(PES_HEADER_BASE)
        .and_then(|v| v.checked_add(hdr_len))
        .and_then(|v| v.checked_add(pay_len))
        .ok_or_else(|| invalid("PES size overflow"))?;
    let space = SECTOR
        .checked_sub(used)
        .ok_or_else(|| invalid("PES pack overflow"))?;

    if space > 0 && space < 6 {
        hdr_len = hdr_len
            .checked_add(space)
            .ok_or_else(|| invalid("PES header overflow"))?;
        if hdr_len > 255 {
            return Err(invalid("PES header too large"));
        }
        opt.extend(std::iter::repeat_n(0xFF, space));
    } else if space >= 6 {
        // Padding is written below.
    }

    let total_pes = PES_HEADER_BASE
        .checked_add(hdr_len)
        .and_then(|v| v.checked_add(pay_len))
        .ok_or_else(|| invalid("PES size overflow"))?;
    let space = SECTOR
        .checked_sub(
            PACK_HEADER_LEN
                .checked_add(total_pes)
                .ok_or_else(|| invalid("PES size overflow"))?,
        )
        .ok_or_else(|| invalid("PES pack overflow"))?;

    write_pack_header(sector, scr)?;

    let packet_length = 3usize
        .checked_add(hdr_len)
        .and_then(|v| v.checked_add(pay_len))
        .ok_or_else(|| invalid("PES packet length overflow"))?;
    let packet_length =
        u16::try_from(packet_length).map_err(|_| invalid("PES packet too large"))?;

    let mut off = PACK_HEADER_LEN;
    put_slice(sector, off, &[0, 0, 1])?;
    off = off
        .checked_add(3)
        .ok_or_else(|| invalid("offset overflow"))?;
    put_u8(sector, off, stream_id)?;
    off = off
        .checked_add(1)
        .ok_or_else(|| invalid("offset overflow"))?;
    put_u16_be(sector, off, packet_length)?;
    off = off
        .checked_add(2)
        .ok_or_else(|| invalid("offset overflow"))?;
    put_u8(sector, off, 0x81)?;
    off = off
        .checked_add(1)
        .ok_or_else(|| invalid("offset overflow"))?;
    put_u8(sector, off, flags2)?;
    off = off
        .checked_add(1)
        .ok_or_else(|| invalid("offset overflow"))?;
    put_u8(sector, off, hdr_len as u8)?;
    off = off
        .checked_add(1)
        .ok_or_else(|| invalid("offset overflow"))?;
    put_slice(sector, off, &opt)?;
    off = off
        .checked_add(hdr_len)
        .ok_or_else(|| invalid("offset overflow"))?;
    put_slice(sector, off, payload)?;
    off = off
        .checked_add(pay_len)
        .ok_or_else(|| invalid("offset overflow"))?;

    if space >= 6 {
        let pad_len = space - 6;
        put_slice(sector, off, &[0, 0, 1, 0xBE])?;
        off = off
            .checked_add(4)
            .ok_or_else(|| invalid("offset overflow"))?;
        put_u16_be(sector, off, pad_len as u16)?;
        off = off
            .checked_add(2)
            .ok_or_else(|| invalid("offset overflow"))?;
        put_fill(sector, off, pad_len, 0xFF)?;
    } else if space != 0 {
        return Err(invalid("bad PES padding space"));
    }

    Ok(())
}

/// Write a 14-byte MPEG-2 pack header.
pub(crate) fn write_pack_header(sector: &mut [u8; SECTOR], scr: i128) -> Result<(), MuxError> {
    put_slice(sector, 0, &[0, 0, 1, 0xBA])?;
    write_scr(sector, 4, scr)?;
    put_slice(sector, 10, &[0x01, 0x89, 0xC3])?;
    put_u8(sector, 13, 0xF8)?;
    Ok(())
}

/// Write one byte.
pub(crate) fn put_u8(sector: &mut [u8; SECTOR], offset: usize, value: u8) -> Result<(), MuxError> {
    let end = offset
        .checked_add(1)
        .ok_or_else(|| invalid("offset overflow"))?;
    let slot = sector
        .get_mut(offset..end)
        .ok_or_else(|| invalid("sector write out of range"))?;
    if let Some(byte) = slot.get_mut(0) {
        *byte = value;
        Ok(())
    } else {
        Err(invalid("sector write out of range"))
    }
}

/// Write a big-endian u16.
pub(crate) fn put_u16_be(
    sector: &mut [u8; SECTOR],
    offset: usize,
    value: u16,
) -> Result<(), MuxError> {
    put_slice(sector, offset, &value.to_be_bytes())
}

/// Write a big-endian u32.
pub(crate) fn put_u32_be(
    sector: &mut [u8; SECTOR],
    offset: usize,
    value: u32,
) -> Result<(), MuxError> {
    put_slice(sector, offset, &value.to_be_bytes())
}

/// Write a byte slice.
pub(crate) fn put_slice(
    sector: &mut [u8; SECTOR],
    offset: usize,
    src: &[u8],
) -> Result<(), MuxError> {
    if src.is_empty() {
        return Ok(());
    }
    let end = offset
        .checked_add(src.len())
        .ok_or_else(|| invalid("offset overflow"))?;
    let dst = sector
        .get_mut(offset..end)
        .ok_or_else(|| invalid("sector write out of range"))?;
    dst.copy_from_slice(src);
    Ok(())
}

/// Fill bytes with a value.
pub(crate) fn put_fill(
    sector: &mut [u8; SECTOR],
    offset: usize,
    len: usize,
    value: u8,
) -> Result<(), MuxError> {
    let mut off = offset;
    for _ in 0..len {
        put_u8(sector, off, value)?;
        off = off
            .checked_add(1)
            .ok_or_else(|| invalid("offset overflow"))?;
    }
    Ok(())
}

fn write_scr(sector: &mut [u8; SECTOR], offset: usize, scr: i128) -> Result<(), MuxError> {
    if scr < 0 {
        return Err(invalid("negative SCR"));
    }
    let base = u64::try_from(scr / 300).map_err(|_| invalid("SCR out of range"))?;
    let ext = u64::try_from(scr % 300).map_err(|_| invalid("SCR extension out of range"))?;
    if base >= (1u64 << 33) {
        return Err(invalid("SCR base out of range"));
    }

    let mut bits: u128 = 0;
    let mut nbits: u32 = 0;
    append_bits(&mut bits, &mut nbits, 1, 2);
    append_bits(&mut bits, &mut nbits, (base >> 30) & 0x7, 3);
    append_bits(&mut bits, &mut nbits, 1, 1);
    append_bits(&mut bits, &mut nbits, (base >> 15) & 0x7FFF, 15);
    append_bits(&mut bits, &mut nbits, 1, 1);
    append_bits(&mut bits, &mut nbits, base & 0x7FFF, 15);
    append_bits(&mut bits, &mut nbits, 1, 1);
    append_bits(&mut bits, &mut nbits, ext & 0x1FF, 9);
    append_bits(&mut bits, &mut nbits, 1, 1);

    write_bits_sector(sector, offset, bits, nbits)
}

fn timestamp_to_u64(value: i64) -> Result<u64, MuxError> {
    if !(0..(1i64 << 33)).contains(&value) {
        return Err(invalid("timestamp out of range"));
    }
    Ok(value as u64)
}

fn append_timestamp_field(bits: &mut u128, nbits: &mut u32, ts: u64) {
    append_bits(bits, nbits, (ts >> 30) & 0x7, 3);
    append_bits(bits, nbits, 1, 1);
    append_bits(bits, nbits, (ts >> 15) & 0x7FFF, 15);
    append_bits(bits, nbits, 1, 1);
    append_bits(bits, nbits, ts & 0x7FFF, 15);
    append_bits(bits, nbits, 1, 1);
}

fn append_bits(bits: &mut u128, nbits: &mut u32, value: u64, n: u32) {
    *bits = (*bits << n) | u128::from(value);
    *nbits = nbits.saturating_add(n);
}

fn write_bits_sector(
    sector: &mut [u8; SECTOR],
    offset: usize,
    bits: u128,
    nbits: u32,
) -> Result<(), MuxError> {
    if !nbits.is_multiple_of(8) {
        return Err(invalid("bit field not byte aligned"));
    }
    let bytes = nbits / 8;
    let mut off = offset;
    for i in 0..bytes {
        let shift = nbits - (8 * (i + 1));
        let byte = ((bits >> shift) & 0xFF) as u8;
        put_u8(sector, off, byte)?;
        off = off
            .checked_add(1)
            .ok_or_else(|| invalid("offset overflow"))?;
    }
    Ok(())
}

fn write_bits_vec(bits: u128, nbits: u32, out: &mut Vec<u8>) -> Result<(), MuxError> {
    if !nbits.is_multiple_of(8) {
        return Err(invalid("bit field not byte aligned"));
    }
    let bytes = nbits / 8;
    for i in 0..bytes {
        let shift = nbits - (8 * (i + 1));
        out.push(((bits >> shift) & 0xFF) as u8);
    }
    Ok(())
}
