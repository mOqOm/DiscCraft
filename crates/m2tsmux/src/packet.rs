//! Transport packet, PES packet, and BDAV source packet construction.

use crate::{M2tsError, SOURCE_PACKET_SIZE};

/// Size of one MPEG-2 transport packet.
pub(crate) const TS_SIZE: usize = 188;

const PAYLOAD_SIZE: usize = 184;
const SYNC_BYTE: u8 = 0x47;
const NULL_PID: u16 = 0x1FFF;

/// Continuity counter state for one PID.
pub(crate) struct Packetizer {
    pid: u16,
    cc: u8,
}

impl Packetizer {
    /// Create a packetizer for `pid`.
    pub(crate) fn new(pid: u16) -> Self {
        Self { pid, cc: 0 }
    }

    /// Split a PES packet into transport packets.
    pub(crate) fn pes_packets(&mut self, pes: &[u8]) -> Vec<[u8; TS_SIZE]> {
        let mut packets = Vec::new();

        for (index, chunk) in pes.chunks(PAYLOAD_SIZE).enumerate() {
            let pusi = index == 0;
            let cc = self.next_cc();

            let packet = if chunk.len() == PAYLOAD_SIZE {
                payload_packet(self.pid, pusi, cc, chunk)
            } else {
                stuffed_packet(self.pid, pusi, cc, chunk)
            };

            packets.push(packet);
        }

        packets
    }

    /// Split a PSI section into transport packets.
    pub(crate) fn section_packets(&mut self, section: &[u8]) -> Vec<[u8; TS_SIZE]> {
        let mut packets = Vec::new();

        if section.is_empty() {
            return packets;
        }

        let first_len = section.len().min(PAYLOAD_SIZE - 1);
        let mut first_payload = Vec::with_capacity(PAYLOAD_SIZE);
        first_payload.push(0x00);
        first_payload.extend(section.iter().take(first_len).copied());

        if first_payload.len() < PAYLOAD_SIZE {
            first_payload.resize(PAYLOAD_SIZE, 0xFF);
        }

        let cc = self.next_cc();
        packets.push(payload_packet(self.pid, true, cc, &first_payload));

        let rest: Vec<u8> = section.iter().skip(first_len).copied().collect();

        for chunk in rest.chunks(PAYLOAD_SIZE) {
            let mut payload = chunk.to_vec();

            if payload.len() < PAYLOAD_SIZE {
                payload.resize(PAYLOAD_SIZE, 0xFF);
            }

            let cc = self.next_cc();
            packets.push(payload_packet(self.pid, false, cc, &payload));
        }

        packets
    }

    /// Return the current continuity counter and advance it.
    fn next_cc(&mut self) -> u8 {
        let cc = self.cc;
        self.cc = (self.cc + 1) & 0x0F;
        cc
    }
}

/// Build one PES packet.
pub(crate) fn pes_packet(
    stream_id: u8,
    pts: u64,
    dts: Option<u64>,
    payload: &[u8],
    bounded: bool,
) -> Result<Vec<u8>, M2tsError> {
    let has_dts = match dts {
        Some(value) => value != pts,
        None => false,
    };

    let timestamp_len = if has_dts { 10 } else { 5 };
    let length = 3 + timestamp_len + payload.len();

    let pes_length = if bounded {
        u16::try_from(length).map_err(|_| M2tsError(format!("PES packet too large: {length}")))?
    } else {
        0
    };

    let mut out = Vec::with_capacity(6 + length);

    out.extend_from_slice(&[0x00, 0x00, 0x01, stream_id]);
    out.push(u8::try_from(pes_length >> 8).unwrap_or(0));
    out.push(u8::try_from(pes_length & 0xFF).unwrap_or(0));

    out.push(0x84);
    out.push(if has_dts { 0xC0 } else { 0x80 });
    out.push(if has_dts { 10 } else { 5 });

    let pts_prefix = if has_dts { 3 } else { 2 };
    write_timestamp(&mut out, pts_prefix, pts);

    if has_dts && let Some(value) = dts {
        write_timestamp(&mut out, 1, value);
    }

    out.extend_from_slice(payload);

    Ok(out)
}

/// Build an adaptation-field-only PCR packet.
pub(crate) fn pcr_packet(pid: u16, pcr_27m: u64) -> [u8; TS_SIZE] {
    let mut packet = [0_u8; TS_SIZE];

    write_header(&mut packet, pid, false, 0b10, 0);
    put_u8(&mut packet, 4, 183);
    put_u8(&mut packet, 5, 0x10);

    let base = (pcr_27m / 300) & 0x1FFF_FFFF;
    let extension = pcr_27m % 300;
    let bits = (base << 15) | (0x3F << 9) | extension;

    for index in 0..6 {
        let shift = 40 - index * 8;
        let byte = u8::try_from((bits >> shift) & 0xFF).unwrap_or(0);
        put_u8(&mut packet, 6 + index, byte);
    }

    put_fill(&mut packet, 12, 176, 0xFF);

    packet
}

/// Build one null transport packet.
pub(crate) fn null_packet() -> [u8; TS_SIZE] {
    let mut packet = [0_u8; TS_SIZE];

    write_header(&mut packet, NULL_PID, false, 0b01, 0);
    put_fill(&mut packet, 4, PAYLOAD_SIZE, 0xFF);

    packet
}

/// Wrap a transport packet in a BDAV source packet.
pub(crate) fn source_packet(ats_27m: u64, ts: &[u8; TS_SIZE]) -> [u8; SOURCE_PACKET_SIZE] {
    let mut packet = [0_u8; SOURCE_PACKET_SIZE];

    let ats = u32::try_from(ats_27m & 0x3FFF_FFFF).unwrap_or(0);
    put_slice(&mut packet, 0, &ats.to_be_bytes());
    put_slice(&mut packet, 4, ts);

    packet
}

/// Build a payload-only transport packet.
fn payload_packet(pid: u16, pusi: bool, cc: u8, payload: &[u8]) -> [u8; TS_SIZE] {
    let mut packet = [0_u8; TS_SIZE];

    write_header(&mut packet, pid, pusi, 0b01, cc);
    put_slice(&mut packet, 4, payload);

    packet
}

/// Build a transport packet whose short payload is padded by an adaptation field.
fn stuffed_packet(pid: u16, pusi: bool, cc: u8, payload: &[u8]) -> [u8; TS_SIZE] {
    let mut packet = [0_u8; TS_SIZE];

    write_header(&mut packet, pid, pusi, 0b11, cc);

    let adaptation_len = PAYLOAD_SIZE - 1 - payload.len();
    put_u8(&mut packet, 4, u8::try_from(adaptation_len).unwrap_or(0));

    if adaptation_len >= 1 {
        put_u8(&mut packet, 5, 0x00);
        put_fill(&mut packet, 6, adaptation_len - 1, 0xFF);
    }

    put_slice(&mut packet, 5 + adaptation_len, payload);

    packet
}

/// Write the four-byte transport packet header.
fn write_header(packet: &mut [u8; TS_SIZE], pid: u16, pusi: bool, afc: u8, cc: u8) {
    let byte0 = SYNC_BYTE;
    let pusi_bit = if pusi { 0x40 } else { 0x00 };
    let byte1 = pusi_bit | u8::try_from((pid >> 8) & 0x1F).unwrap_or(0);
    let byte2 = u8::try_from(pid & 0xFF).unwrap_or(0);
    let byte3 = ((afc & 0x03) << 4) | (cc & 0x0F);

    put_u8(packet, 0, byte0);
    put_u8(packet, 1, byte1);
    put_u8(packet, 2, byte2);
    put_u8(packet, 3, byte3);
}

/// Write one 33-bit PTS/DTS field.
fn write_timestamp(out: &mut Vec<u8>, prefix: u8, value: u64) {
    let byte0 =
        u8::try_from((u64::from(prefix) << 4) | (((value >> 30) & 0x07) << 1) | 1).unwrap_or(0);
    let byte1 = u8::try_from((value >> 22) & 0xFF).unwrap_or(0);
    let byte2 = u8::try_from((((value >> 15) & 0x7F) << 1) | 1).unwrap_or(0);
    let byte3 = u8::try_from((value >> 7) & 0xFF).unwrap_or(0);
    let byte4 = u8::try_from(((value & 0x7F) << 1) | 1).unwrap_or(0);

    out.extend_from_slice(&[byte0, byte1, byte2, byte3, byte4]);
}

/// Put one byte if the offset is valid.
fn put_u8(packet: &mut [u8], offset: usize, value: u8) {
    if let Some(byte) = packet.get_mut(offset) {
        *byte = value;
    }
}

/// Put a slice if the destination range is valid.
fn put_slice(packet: &mut [u8], offset: usize, value: &[u8]) {
    let end = offset.saturating_add(value.len());

    if let Some(dst) = packet.get_mut(offset..end) {
        dst.copy_from_slice(value);
    }
}

/// Fill a range with one byte if the destination range is valid.
fn put_fill(packet: &mut [u8], offset: usize, len: usize, value: u8) {
    let end = offset.saturating_add(len);

    if let Some(dst) = packet.get_mut(offset..end) {
        dst.fill(value);
    }
}

#[cfg(test)]
mod tests {
    use super::{Packetizer, TS_SIZE, null_packet, pcr_packet, pes_packet, source_packet};
    use crate::M2tsError;

    #[test]
    fn pes_pts_only_header() -> Result<(), M2tsError> {
        let pes = pes_packet(0xE0, 90_000, None, b"x", false)?;

        assert_eq!(
            &pes[..14],
            &[
                0x00, 0x00, 0x01, 0xE0, 0x00, 0x00, 0x84, 0x80, 0x05, 0x21, 0x00, 0x05, 0xBF, 0x21
            ]
        );

        Ok(())
    }

    #[test]
    fn pes_pts_dts_header() -> Result<(), M2tsError> {
        let pes = pes_packet(0xE0, 90_000, Some(80_000), b"x", true)?;

        assert_eq!(pes[6], 0x84);
        assert_eq!(pes[7], 0xC0);
        assert_eq!(pes[8], 10);

        let length = (u16::from(pes[4]) << 8) | u16::from(pes[5]);
        assert_eq!(length, 14);

        Ok(())
    }

    #[test]
    fn pcr_packet_roundtrip() {
        let value = 123_456_789_u64;
        let packet = pcr_packet(0x1001, value);

        assert_eq!(packet[0], 0x47);
        assert_eq!((packet[3] >> 4) & 0x03, 0b10);
        assert_eq!(packet[4], 183);
        assert_eq!(packet[5], 0x10);

        let mut bits = 0_u64;
        for index in 0..6 {
            bits = (bits << 8) | u64::from(packet[6 + index]);
        }

        let base = bits >> 15;
        let extension = bits & 0x1FF;

        assert_eq!(base * 300 + extension, value);
    }

    #[test]
    fn pes_packets_split_and_stuff() {
        let mut packetizer = Packetizer::new(0x1011);
        let packets = packetizer.pes_packets(&[0_u8; 400]);

        assert_eq!(packets.len(), 3);

        for (index, packet) in packets.iter().enumerate() {
            assert_eq!(usize::from(packet[3] & 0x0F), index);
        }

        assert_eq!((packets[2][3] >> 4) & 0x03, 0b11);
        assert_eq!(packets[2][4], 151);
    }

    #[test]
    fn source_packet_masks_ats() {
        let ts = null_packet();
        let source = source_packet((1 << 30) + 5, &ts);

        assert_eq!(&source[..4], &[0x00, 0x00, 0x00, 0x05]);
        assert_eq!(&source[4..], &ts[..]);
    }

    #[test]
    fn packetizer_section_pads_last_packet() {
        let mut packetizer = Packetizer::new(0);
        let packets = packetizer.section_packets(&[0xAA; 10]);

        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0][4], 0x00);
        assert_eq!(packets[0][5], 0xAA);
        assert_eq!(packets[0][TS_SIZE - 1], 0xFF);
    }
}
