//! BDAV m2ts multiplexer.

use crate::packet::{Packetizer, TS_SIZE, null_packet, pcr_packet, pes_packet, source_packet};
use crate::psi::{PmtStream, pat_section, pmt_section};
use crate::{
    ALIGNED_UNIT_PACKETS, EpEntry, EsInput, M2tsError, M2tsLayout, MuxConfig, PCR_PID, PMT_PID,
    StreamKind, UnitData,
};

use std::collections::VecDeque;
use std::io::Write;

const PSI_INTERVAL: u64 = 2_700_000;
const PCR_INTERVAL: u64 = 1_080_000;
const VIDEO_WINDOW: u64 = 13_500_000;
const AUDIO_WINDOW: u64 = 6_750_000;
const GRAPHICS_WINDOW: u64 = 27_000_000;
const TICKS_PER_90KHZ: u64 = 300;
const PACKET_TIME_NUMERATOR: u64 = 192 * 8 * 27_000_000;

/// One scheduled PES packet: built from its unit when it is written, so a clip's units need not all be in memory.
struct PesItem {
    /// Its unit in the stream.
    unit: usize,
    /// The PES packet's size in bytes.
    pes_len: usize,
    pts: u64,
    /// Its decoding time (27 MHz): a packet sent later is late.
    deadline: u64,
    /// When it must start to be sent (27 MHz), the order the streams are served in: its decoding time, or earlier
    /// for video that a burst ahead of it needs sent early.
    send_by: u64,
    earliest: u64,
    random_access: bool,
}

/// The share of the mux rate video is planned at when it is sent ahead of a burst (the rest is left to audio,
/// graphics and PSI). At 48 Mbit/s, 36: above a Blu-ray's video rate, so the decoder's buffer holds no more than the
/// encoder's VBV model let it.
const VIDEO_SHARE_NUMERATOR: u64 = 3;
const VIDEO_SHARE_DENOMINATOR: u64 = 4;

/// Transport stream payload bytes per packet.
const TS_PAYLOAD: u64 = 184;

/// Clock and packetizer state for one multiplexing run.
struct Scheduler<'a> {
    t: u64,
    spn: u32,
    packet_time: u64,
    late: u32,
    next_psi_time: u64,
    next_pcr_time: u64,
    pat: Packetizer,
    pmt: Packetizer,
    pmt_section: Vec<u8>,
    streams: Vec<Packetizer>,
    out: &'a mut dyn Write,
}

impl<'a> Scheduler<'a> {
    /// Write one source packet at the current clock and advance the clock.
    fn write_source(&mut self, ts: &[u8; TS_SIZE]) -> Result<(), M2tsError> {
        let source = source_packet(self.t, ts);

        self.out
            .write_all(&source)
            .map_err(|error| M2tsError(format!("write failed: {error}")))?;

        self.t = self
            .t
            .checked_add(self.packet_time)
            .ok_or_else(|| M2tsError("clock overflow".to_string()))?;

        self.spn = self
            .spn
            .checked_add(1)
            .ok_or_else(|| M2tsError("source packet count overflow".to_string()))?;

        Ok(())
    }

    /// Write PAT packets.
    fn write_pat(&mut self) -> Result<(), M2tsError> {
        let section = pat_section(1, PMT_PID);
        let packets = self.pat.section_packets(&section);

        for packet in packets {
            self.write_source(&packet)?;
        }

        Ok(())
    }

    /// Write PMT packets.
    fn write_pmt(&mut self) -> Result<(), M2tsError> {
        let packets = self.pmt.section_packets(&self.pmt_section);

        for packet in packets {
            self.write_source(&packet)?;
        }

        Ok(())
    }

    /// Write one PAT/PMT pair and schedule the next pair.
    fn write_pat_pmt(&mut self) -> Result<(), M2tsError> {
        self.write_pat()?;
        self.write_pmt()?;

        self.next_psi_time = self
            .t
            .checked_add(PSI_INTERVAL)
            .ok_or_else(|| M2tsError("PSI schedule overflow".to_string()))?;

        Ok(())
    }

    /// Write one PCR packet and schedule the next PCR.
    fn write_pcr(&mut self) -> Result<(), M2tsError> {
        let pcr_time = self.t;
        let packet = pcr_packet(PCR_PID, pcr_time);

        self.write_source(&packet)?;

        self.next_pcr_time = pcr_time
            .checked_add(PCR_INTERVAL)
            .ok_or_else(|| M2tsError("PCR schedule overflow".to_string()))?;

        Ok(())
    }

    /// Write PSI and PCR packets if their intervals have elapsed.
    fn write_due_control(&mut self) -> Result<(), M2tsError> {
        if self.t >= self.next_psi_time {
            self.write_pat_pmt()?;
        }

        if self.t >= self.next_pcr_time {
            self.write_pcr()?;
        }

        Ok(())
    }
}

/// Multiplex elementary streams into a BDAV m2ts clip.
pub fn mux_m2ts(
    streams: &[EsInput],
    config: &MuxConfig,
    out: &mut dyn std::io::Write,
) -> Result<M2tsLayout, M2tsError> {
    mux_m2ts_with(streams, &[], config, out)
}

/// [`mux_m2ts`], the units of a stream with a reader in `readers` (one entry per stream, `None` or missing for one
/// whose units hold their data) read from it as they are written: a long HD title's video is then never in memory
/// at once.
pub fn mux_m2ts_with(
    streams: &[EsInput],
    readers: &[Option<&dyn UnitData>],
    config: &MuxConfig,
    out: &mut dyn std::io::Write,
) -> Result<M2tsLayout, M2tsError> {
    validate(streams, config)?;
    let reader = |stream: usize| readers.get(stream).copied().flatten();

    let packet_time = packet_time(config.mux_rate)?;
    let pmt_section = build_pmt_section(streams)?;
    let (queues, start) = build_queues(streams, readers, packet_time)?;
    let start = start.unwrap_or(0);

    let (first_video_index, first_video_pid) = first_video(streams);
    let stream_packetizers = streams
        .iter()
        .map(|stream| Packetizer::new(stream.pid))
        .collect();

    let mut scheduler = Scheduler {
        t: start,
        spn: 0,
        packet_time,
        late: 0,
        next_psi_time: start,
        next_pcr_time: start,
        pat: Packetizer::new(0),
        pmt: Packetizer::new(PMT_PID),
        pmt_section,
        streams: stream_packetizers,
        out,
    };

    // PAT / PMT / PCR are written when due, and always right before a random access video unit, so the clip starts with
    // a PAT and every EP-map entry points at a PAT that is followed by everything a decoder needs.
    let mut queues = queues;
    let mut ep_map = Vec::new();

    while let Some(stream_index) = next_stream(&queues) {
        let queue = queues
            .get_mut(stream_index)
            .ok_or_else(|| M2tsError("missing stream queue".to_string()))?;

        let item = queue
            .pop_front()
            .ok_or_else(|| M2tsError("missing scheduled PES packet".to_string()))?;

        let record_ep = first_video_index == Some(stream_index) && item.random_access;

        let pes = build_pes(streams, stream_index, item.unit, reader(stream_index))?;
        let packets = scheduler
            .streams
            .get_mut(stream_index)
            .ok_or_else(|| M2tsError("missing stream packetizer".to_string()))?
            .pes_packets(&pes);

        write_pes_item(&mut scheduler, &item, packets, record_ep, &mut ep_map)?;
    }

    while !scheduler.spn.is_multiple_of(ALIGNED_UNIT_PACKETS) {
        let packet = null_packet();
        scheduler.write_source(&packet)?;
    }

    Ok(M2tsLayout {
        source_packets: scheduler.spn,
        ep_map,
        ep_pid: first_video_pid,
        late_packets: scheduler.late,
    })
}

/// Validate multiplexer inputs.
fn validate(streams: &[EsInput], config: &MuxConfig) -> Result<(), M2tsError> {
    if streams.is_empty() {
        return Err(M2tsError("at least one stream is required".to_string()));
    }

    if config.mux_rate < 1_000_000 {
        return Err(M2tsError("mux_rate must be at least 1_000_000".to_string()));
    }

    let mut seen = Vec::new();

    for stream in streams {
        if stream.pid == 0
            || stream.pid == PMT_PID
            || stream.pid == PCR_PID
            || stream.pid == 0x1FFF
            || stream.pid > 0x1FFF
        {
            return Err(M2tsError(format!("invalid PID: {:#05x}", stream.pid)));
        }

        if seen.contains(&stream.pid) {
            return Err(M2tsError(format!("duplicate PID: {:#05x}", stream.pid)));
        }

        seen.push(stream.pid);
    }

    Ok(())
}

/// Compute the source packet duration in 27 MHz ticks.
fn packet_time(mux_rate: u32) -> Result<u64, M2tsError> {
    let rate = u64::from(mux_rate);

    PACKET_TIME_NUMERATOR
        .checked_add(rate - 1)
        .ok_or_else(|| M2tsError("mux rate overflow".to_string()))?
        .checked_div(rate)
        .ok_or_else(|| M2tsError("mux rate must be non-zero".to_string()))
}

/// Build the PMT section bytes.
fn build_pmt_section(streams: &[EsInput]) -> Result<Vec<u8>, M2tsError> {
    let pmt_streams: Vec<PmtStream> = streams
        .iter()
        .map(|stream| PmtStream {
            stream_type: stream.kind.stream_type(),
            pid: stream.pid,
        })
        .collect();

    pmt_section(1, PCR_PID, &pmt_streams)
}

/// One unit's PES packet, its data from `reader` when the stream has one.
fn build_pes(
    streams: &[EsInput],
    stream: usize,
    unit: usize,
    reader: Option<&dyn UnitData>,
) -> Result<Vec<u8>, M2tsError> {
    let input = streams
        .get(stream)
        .ok_or_else(|| M2tsError("missing stream".to_string()))?;
    let es = input
        .units
        .get(unit)
        .ok_or_else(|| M2tsError("missing unit".to_string()))?;
    let read;
    let payload: &[u8] = match reader {
        Some(reader) => {
            read = reader
                .read(unit)
                .map_err(|error| M2tsError(format!("reading unit {unit}: {error}")))?;
            &read
        }
        None => &es.data,
    };
    pes_packet(
        input.kind.stream_id(),
        es.pts,
        Some(es.dts),
        payload,
        !input.kind.is_video(),
    )
}

/// Build per-stream PES queues and the initial clock (`packet_time`: one source packet at the mux rate, 27 MHz).
fn build_queues(
    streams: &[EsInput],
    readers: &[Option<&dyn UnitData>],
    packet_time: u64,
) -> Result<(Vec<VecDeque<PesItem>>, Option<u64>), M2tsError> {
    let mut queues = Vec::with_capacity(streams.len());

    for (index, stream) in streams.iter().enumerate() {
        let mut queue = VecDeque::new();
        let reader = readers.get(index).copied().flatten();

        for (number, unit) in stream.units.iter().enumerate() {
            let bounded = !stream.kind.is_video();
            // The PES header (made with no payload) and the payload, which is read only when it is written.
            let header = pes_packet(
                stream.kind.stream_id(),
                unit.pts,
                Some(unit.dts),
                &[],
                bounded,
            )?;
            let payload = match reader {
                Some(reader) => reader.len(number),
                None => unit.data.len(),
            };
            if bounded && header.len() + payload > usize::from(u16::MAX) + 6 {
                return Err(M2tsError(format!(
                    "PES packet too large: {}",
                    header.len() + payload
                )));
            }

            let deadline = unit
                .dts
                .checked_mul(TICKS_PER_90KHZ)
                .ok_or_else(|| M2tsError("deadline overflow".to_string()))?;

            queue.push_back(PesItem {
                unit: number,
                pes_len: header.len() + payload,
                pts: unit.pts,
                deadline,
                send_by: deadline,
                earliest: 0,
                random_access: unit.random_access,
            });
        }

        if stream.kind.is_video() {
            plan_video_ahead(&mut queue, packet_time);
        }
        for item in queue.iter_mut() {
            item.earliest = item.send_by.saturating_sub(window_ticks(stream.kind));
        }

        queues.push(queue);
    }

    let start = queues
        .iter()
        .filter_map(|queue| queue.iter().map(|item| item.earliest).min())
        .min();
    Ok((queues, start))
}

/// Moves each video PES's `send_by` early enough that, sent one after another at the video share of the mux rate,
/// every one is in by its decoding time: walking back from the last, a PES must start a send time before the
/// earlier of its own deadline and the start of the next. A burst of large pictures (which an H.264 encoder's VBV
/// buffer allows) is so sent ahead of time instead of arriving late.
fn plan_video_ahead(queue: &mut VecDeque<PesItem>, packet_time: u64) {
    let video_packet_time =
        packet_time.saturating_mul(VIDEO_SHARE_DENOMINATOR) / VIDEO_SHARE_NUMERATOR;
    let mut next_start = u64::MAX;
    for item in queue.iter_mut().rev() {
        let packets = u64::try_from(item.pes_len)
            .unwrap_or(u64::MAX)
            .div_ceil(TS_PAYLOAD);
        let send_time = packets.saturating_mul(video_packet_time);
        let finish_by = item.deadline.min(next_start);
        item.send_by = finish_by.saturating_sub(send_time);
        next_start = item.send_by;
    }
}

/// Return the scheduling window for one stream kind.
fn window_ticks(kind: StreamKind) -> u64 {
    match kind {
        StreamKind::Mpeg2Video | StreamKind::H264Video => VIDEO_WINDOW,
        StreamKind::Lpcm | StreamKind::Ac3 => AUDIO_WINDOW,
        StreamKind::Pgs | StreamKind::Igs => GRAPHICS_WINDOW,
    }
}

/// Return the first video stream index and PID.
fn first_video(streams: &[EsInput]) -> (Option<usize>, u16) {
    for (index, stream) in streams.iter().enumerate() {
        if stream.kind.is_video() {
            return (Some(index), stream.pid);
        }
    }

    (None, 0)
}

/// Return the stream index whose next PES must be sent first.
fn next_stream(queues: &[VecDeque<PesItem>]) -> Option<usize> {
    let mut best: Option<(usize, u64)> = None;

    for (index, queue) in queues.iter().enumerate() {
        if let Some(item) = queue.front() {
            match best {
                None => best = Some((index, item.send_by)),
                Some((_, best_send_by)) => {
                    if item.send_by < best_send_by {
                        best = Some((index, item.send_by));
                    }
                }
            }
        }
    }

    best.map(|(index, _)| index)
}

/// Write one PES item as transport packets.
fn write_pes_item(
    scheduler: &mut Scheduler<'_>,
    item: &PesItem,
    packets: Vec<[u8; TS_SIZE]>,
    record_ep: bool,
    ep_map: &mut Vec<EpEntry>,
) -> Result<(), M2tsError> {
    let mut first = true;

    for ts in packets {
        if first && scheduler.t < item.earliest {
            scheduler.t = item.earliest;
        }

        if first && record_ep {
            ep_map.push(EpEntry {
                pts: item.pts,
                spn: scheduler.spn,
            });
            scheduler.write_pat_pmt()?;
            scheduler.write_pcr()?;
        }

        scheduler.write_due_control()?;

        if scheduler.t > item.deadline {
            scheduler.late = scheduler
                .late
                .checked_add(1)
                .ok_or_else(|| M2tsError("late packet count overflow".to_string()))?;
        }

        scheduler.write_source(&ts)?;
        first = false;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::mux_m2ts;
    use crate::{
        AUDIO_PID, EsInput, EsUnit, M2tsError, MuxConfig, PCR_PID, PMT_PID, SOURCE_PACKET_SIZE,
        StreamKind, VIDEO_PID,
    };

    use std::collections::{HashMap, HashSet};

    #[test]
    fn mux_basic_clip() -> Result<(), M2tsError> {
        let video_units: Vec<EsUnit> = (0..30)
            .map(|index| {
                let dts = 45_000 + 3_003 * index;

                EsUnit {
                    pts: dts,
                    dts,
                    data: vec![0_u8; 1_000],
                    random_access: index % 15 == 0,
                }
            })
            .collect();

        let audio_units: Vec<EsUnit> = (0..30)
            .map(|index| {
                let pts = 45_000 + 450 * index;

                EsUnit {
                    pts,
                    dts: pts,
                    data: vec![0_u8; 964],
                    random_access: true,
                }
            })
            .collect();

        let streams = vec![
            EsInput {
                pid: VIDEO_PID,
                kind: StreamKind::Mpeg2Video,
                units: video_units,
            },
            EsInput {
                pid: AUDIO_PID,
                kind: StreamKind::Lpcm,
                units: audio_units,
            },
        ];

        let config = MuxConfig::default();
        let mut out = Vec::new();

        let layout = mux_m2ts(&streams, &config, &mut out)?;

        assert_eq!(out.len() % 6_144, 0);
        assert_eq!(
            usize::try_from(layout.source_packets).unwrap_or(0) * SOURCE_PACKET_SIZE,
            out.len()
        );
        assert_eq!(layout.late_packets, 0);
        assert_eq!(layout.ep_map.len(), 2);
        assert_eq!(layout.ep_pid, VIDEO_PID);

        let mut pids = HashSet::new();

        for (spn, chunk) in out.as_chunks::<SOURCE_PACKET_SIZE>().0.iter().enumerate() {
            assert_eq!(chunk[4], 0x47);

            let pid = (u16::from(chunk[5] & 0x1F) << 8) | u16::from(chunk[6]);
            pids.insert(pid);

            if spn == 0 {
                assert_eq!(pid, 0);
                assert_ne!(chunk[5] & 0x40, 0);
            }

            if spn == 1 {
                assert_eq!(pid, PMT_PID);
            }
        }

        assert!(pids.contains(&VIDEO_PID));
        assert!(pids.contains(&AUDIO_PID));
        assert!(pids.contains(&PCR_PID));

        for entry in &layout.ep_map {
            let spn = usize::try_from(entry.spn).unwrap_or(0);
            let offset = spn * SOURCE_PACKET_SIZE;
            let chunk = out
                .get(offset..offset + SOURCE_PACKET_SIZE)
                .ok_or_else(|| M2tsError("EP SPN out of range".to_string()))?;

            // Every entry point starts with a PAT (PID 0, PUSI) written right before the I picture.
            let pid = (u16::from(chunk[5] & 0x1F) << 8) | u16::from(chunk[6]);
            assert_eq!(pid, 0);
            assert_ne!(chunk[5] & 0x40, 0);
        }
        assert_eq!(layout.ep_map.first().map(|entry| entry.spn), Some(0));

        let mut last_cc: HashMap<u16, u8> = HashMap::new();

        for chunk in out.as_chunks::<SOURCE_PACKET_SIZE>().0 {
            let pid = (u16::from(chunk[5] & 0x1F) << 8) | u16::from(chunk[6]);

            if pid == 0x1FFF || pid == PCR_PID {
                continue;
            }

            let afc = (chunk[7] >> 4) & 0x03;

            if afc == 0b01 || afc == 0b11 {
                let cc = chunk[7] & 0x0F;

                if let Some(previous) = last_cc.get(&pid) {
                    assert_eq!(cc, (previous + 1) & 0x0F);
                }

                last_cc.insert(pid, cc);
            }
        }

        Ok(())
    }

    /// A burst of large pictures (an H.264 encoder's VBV buffer allows one: 48 Mbit drawn down at once) is sent ahead
    /// of time, so nothing is late although no half-second window could carry it at 48 Mbit/s.
    #[test]
    fn a_video_burst_is_sent_ahead_and_nothing_is_late() -> Result<(), M2tsError> {
        // One second at 23.976 with 24 Mbit/s on average, then twelve pictures of 500 kB (48 Mbit in half a second).
        let frame = 3_754_u64;
        let video_units: Vec<EsUnit> = (0..48_u64)
            .map(|index| {
                let dts = 90_000 + frame * index;
                let size = if (24..36).contains(&index) {
                    500_000
                } else {
                    125_000
                };
                EsUnit {
                    pts: dts + frame,
                    dts,
                    data: vec![0_u8; size],
                    random_access: index % 24 == 0,
                }
            })
            .collect();
        let audio_units: Vec<EsUnit> = (0..64_u64)
            .map(|index| {
                let pts = 90_000 + 2_880 * index;
                EsUnit {
                    pts,
                    dts: pts,
                    data: vec![0_u8; 640],
                    random_access: true,
                }
            })
            .collect();
        let streams = vec![
            EsInput {
                pid: VIDEO_PID,
                kind: StreamKind::H264Video,
                units: video_units,
            },
            EsInput {
                pid: AUDIO_PID,
                kind: StreamKind::Ac3,
                units: audio_units,
            },
        ];

        let mut out = Vec::new();
        let layout = mux_m2ts(&streams, &MuxConfig::default(), &mut out)?;
        assert_eq!(layout.late_packets, 0);
        assert_eq!(layout.ep_map.len(), 2);
        Ok(())
    }
}
