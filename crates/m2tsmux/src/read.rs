//! Reading one stream's PES payloads back out of a BDAV m2ts clip: ISO/IEC 13818-1 transport packets, each after a
//! 4-byte `TP_extra_header` (192-byte source packets). The Blu-ray preview reads the menus' interactive graphics
//! (one segment per PES packet, as [`crate::mux_m2ts`] writes them) from the built clips with it.

use crate::{M2tsError, SOURCE_PACKET_SIZE};

/// Transport packet sync byte.
const SYNC: u8 = 0x47;
/// `TP_extra_header` length before each transport packet.
const EXTRA_HEADER: usize = 4;
/// PES start code prefix and the `PES_packet_length` field: the fixed part before the optional header.
const PES_FIXED: usize = 6;

/// The payload of every PES packet of `pid`, in order: the transport packets' payloads joined from one
/// `payload_unit_start_indicator` to the next, without the PES headers. A trailing partial source packet is ignored;
/// a PES packet that the clip cuts off is left out.
///
/// # Errors
///
/// A packet without the sync byte, an adaptation field longer than its packet, or a PES packet without its start
/// code or with a header longer than the packet.
pub fn pes_payloads(data: &[u8], pid: u16) -> Result<Vec<Vec<u8>>, M2tsError> {
    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut current: Option<Vec<u8>> = None;
    let (source_packets, _) = data.as_chunks::<SOURCE_PACKET_SIZE>();
    for source in source_packets {
        let Some(ts) = source.get(EXTRA_HEADER..) else {
            continue;
        };
        let &[sync, b1, b2, b3] = ts
            .first_chunk::<4>()
            .ok_or_else(|| err("short transport packet"))?;
        if sync != SYNC {
            return Err(err("transport packet without its sync byte"));
        }
        let packet_pid = (u16::from(b1 & 0x1F) << 8) | u16::from(b2);
        if packet_pid != pid {
            continue;
        }
        let unit_start = b1 & 0x40 != 0;
        let control = (b3 >> 4) & 0b11;
        let mut start = 4_usize;
        if control & 0b10 != 0 {
            let length = ts
                .get(4)
                .copied()
                .ok_or_else(|| err("short adaptation field"))?;
            start = start
                .checked_add(1 + usize::from(length))
                .ok_or_else(|| err("adaptation field overflow"))?;
        }
        if control & 0b01 == 0 {
            continue;
        }
        let payload = ts
            .get(start..)
            .ok_or_else(|| err("adaptation field longer than its packet"))?;
        if unit_start {
            packets.extend(current.take());
            current = Some(payload.to_vec());
        } else if let Some(open) = current.as_mut() {
            open.extend_from_slice(payload);
        }
    }
    packets.extend(current);

    let mut out = Vec::with_capacity(packets.len());
    for packet in &packets {
        if let Some(payload) = pes_payload(packet)? {
            out.push(payload.to_vec());
        }
    }
    Ok(out)
}

/// The payload of one PES packet; `None` when the packet is cut off before its stated length.
fn pes_payload(pes: &[u8]) -> Result<Option<&[u8]>, M2tsError> {
    let fixed = pes
        .get(..PES_FIXED)
        .ok_or_else(|| err("short PES packet"))?;
    if fixed.get(..3) != Some(&[0x00, 0x00, 0x01][..]) {
        return Err(err("PES packet without its start code"));
    }
    let stream_id = fixed.get(3).copied().unwrap_or(0);
    let length = usize::from(u16::from_be_bytes([
        fixed.get(4).copied().unwrap_or(0),
        fixed.get(5).copied().unwrap_or(0),
    ]));
    // Padding and the private stream 2 / ECM / EMM / directory streams carry no optional header.
    let start = if matches!(
        stream_id,
        0xBC | 0xBE | 0xBF | 0xF0 | 0xF1 | 0xF2 | 0xF8 | 0xFF
    ) {
        PES_FIXED
    } else {
        let header_length = pes.get(8).copied().ok_or_else(|| err("short PES header"))?;
        PES_FIXED + 3 + usize::from(header_length)
    };
    let end = if length == 0 {
        pes.len()
    } else {
        PES_FIXED.saturating_add(length)
    };
    if end > pes.len() {
        return Ok(None);
    }
    pes.get(start..end)
        .map(Some)
        .ok_or_else(|| err("PES header longer than its packet"))
}

fn err(message: &str) -> M2tsError {
    M2tsError(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::pes_payloads;
    use crate::{EsInput, EsUnit, IG_PID, MuxConfig, StreamKind, VIDEO_PID, mux_m2ts};

    /// What the multiplexer wrote for the graphics stream comes back, one segment per PES packet, among the video.
    #[test]
    fn the_graphics_segments_come_back_out_of_the_clip() -> Result<(), Box<dyn std::error::Error>> {
        let segments: Vec<Vec<u8>> = vec![
            vec![0x18, 0, 3, 1, 2, 3],
            vec![0x80, 0, 0],
            (0..400_u16).map(|n| (n % 251) as u8).collect(),
        ];
        let video = EsInput {
            pid: VIDEO_PID,
            kind: StreamKind::Mpeg2Video,
            units: vec![EsUnit {
                pts: 90_000,
                dts: 90_000,
                data: vec![0, 0, 1, 0xB3, 1, 2, 3, 4],
                random_access: true,
            }],
        };
        let graphics = EsInput {
            pid: IG_PID,
            kind: StreamKind::Igs,
            units: segments
                .iter()
                .map(|data| EsUnit {
                    pts: 80_000,
                    dts: 80_000,
                    data: data.clone(),
                    random_access: false,
                })
                .collect(),
        };
        let mut clip = Vec::new();
        let _ = mux_m2ts(&[video, graphics], &MuxConfig::default(), &mut clip)?;
        assert_eq!(pes_payloads(&clip, IG_PID)?, segments);
        assert!(pes_payloads(&clip, 0x1FFE)?.is_empty());
        // Broken input does not panic.
        assert!(pes_payloads(&[0_u8; 192], IG_PID).is_err());
        assert!(pes_payloads(&clip[..100], IG_PID)?.is_empty());
        Ok(())
    }
}
