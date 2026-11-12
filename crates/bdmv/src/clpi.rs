//! Clip Information (CLPI) files.
//!
//! This module implements the subset of `CLIPINF/*.clpi` needed by DiscCraft:
//! one ATC sequence, one STC sequence, one program, and one EP-map stream.

use crate::{BdmvError, Reader, Stream, StreamCoding, VERSION, len_u32, patch_u32};

/// One entry point: an I picture or other random access point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EpEntry {
    /// Presentation time stamp in 90 kHz units. Only bits 9..32 are stored.
    pub pts: u64,
    /// Source packet number. Only bits 17..31 are stored in the coarse entry.
    pub spn: u32,
}

/// A whole clip information file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClipInfo {
    /// TS recording rate as stored in the file.
    pub ts_recording_rate: u32,
    /// Number of source packets.
    pub source_packets: u32,
    /// PCR PID.
    pub pcr_pid: u16,
    /// Program map PID.
    pub pmt_pid: u16,
    /// Presentation start time in 45 kHz units.
    pub presentation_start: u32,
    /// Presentation end time in 45 kHz units.
    pub presentation_end: u32,
    /// Program streams in PMT order.
    pub streams: Vec<Stream>,
    /// PID of the stream carrying the EP map.
    pub ep_pid: u16,
    /// Entry point map.
    pub ep_map: Vec<EpEntry>,
}

/// Write a clip information file.
pub fn write_clpi(clip: &ClipInfo) -> Result<Vec<u8>, BdmvError> {
    let mut buf = Vec::new();
    let positions = write_header(&mut buf)?;

    write_clip_info(&mut buf, clip)?;

    let seq_addr = len_u32(buf.len())?;
    patch_u32(&mut buf, positions.seq, seq_addr)?;
    write_sequence_info(&mut buf, clip)?;

    let prog_addr = len_u32(buf.len())?;
    patch_u32(&mut buf, positions.prog, prog_addr)?;
    write_program_info(&mut buf, clip)?;

    let cpi_addr = len_u32(buf.len())?;
    patch_u32(&mut buf, positions.cpi, cpi_addr)?;
    write_cpi(&mut buf, clip)?;

    let mark_addr = len_u32(buf.len())?;
    patch_u32(&mut buf, positions.mark, mark_addr)?;
    write_clip_mark(&mut buf)?;

    Ok(buf)
}

/// Read a clip information file written by [`write_clpi`].
pub fn read_clpi(data: &[u8]) -> Result<ClipInfo, BdmvError> {
    let mut r = Reader::at(data, 0);
    if r.bytes(4)? != b"HDMV" {
        return Err(BdmvError("not a clip information file".to_string()));
    }
    r.skip(4)?;

    let seq_addr = r.u32()?;
    let prog_addr = r.u32()?;
    let cpi_addr = r.u32()?;
    let mark_addr = r.u32()?;
    let _ext_addr = r.u32()?;
    r.skip(12)?;
    check_block(data, mark_addr)?;

    let (ts_recording_rate, source_packets) = read_clip_info(&mut r)?;
    let (pcr_pid, presentation_start, presentation_end) = read_sequence_info(data, seq_addr)?;
    let (pmt_pid, streams) = read_program_info(data, prog_addr)?;
    let (ep_pid, ep_map) = read_cpi(data, cpi_addr)?;

    Ok(ClipInfo {
        ts_recording_rate,
        source_packets,
        pcr_pid,
        pmt_pid,
        presentation_start,
        presentation_end,
        streams,
        ep_pid,
        ep_map,
    })
}

/// Check that the length-prefixed block at `addr` lies inside `data`.
fn check_block(data: &[u8], addr: u32) -> Result<(), BdmvError> {
    let start = usize::try_from(addr).map_err(|_| BdmvError("address overflow".to_string()))?;
    let mut r = Reader::at(data, start);
    let len = usize::try_from(r.u32()?).map_err(|_| BdmvError("length overflow".to_string()))?;
    r.skip(len)
}

struct HeaderPositions {
    seq: usize,
    prog: usize,
    cpi: usize,
    mark: usize,
}

struct CoarseGroup {
    first_fine_index: u32,
    pts_coarse: u32,
    spn_coarse: u32,
}

struct Coarse {
    ref_to_fine: u32,
    pts_coarse: u32,
    spn_coarse: u32,
}

struct StreamPidEntry {
    ep_pid: u16,
    coarse_count: u16,
    fine_count: u32,
    epmap_addr: u32,
}

#[derive(Debug, Clone)]
struct BitWriter {
    out: Vec<u8>,
    current: u8,
    used: usize,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            current: 0,
            used: 0,
        }
    }

    fn write(&mut self, value: u64, bits: usize) -> Result<(), BdmvError> {
        if bits == 0 {
            return Ok(());
        }
        if bits > 64 {
            return Err(BdmvError("bit field too wide".to_string()));
        }
        if bits < 64 && (value >> bits) != 0 {
            return Err(BdmvError("value does not fit in bit field".to_string()));
        }

        let mut remaining = bits;
        while remaining > 0 {
            let free = 8 - self.used;
            let take = if remaining < free { remaining } else { free };
            let shift = remaining - take;
            let mask = (1u64 << take) - 1;
            let chunk = u8::try_from((value >> shift) & mask)
                .map_err(|_| BdmvError("bit chunk overflow".to_string()))?;

            self.current |= chunk << (free - take);
            self.used += take;
            remaining -= take;

            if self.used == 8 {
                self.out.push(self.current);
                self.current = 0;
                self.used = 0;
            }
        }

        Ok(())
    }

    fn finish(mut self) -> Vec<u8> {
        if self.used != 0 {
            self.out.push(self.current);
        }
        self.out
    }
}

#[derive(Debug, Clone)]
struct BitReader<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn at(data: &'a [u8]) -> Self {
        Self { data, bit: 0 }
    }

    fn read(&mut self, bits: usize) -> Result<u64, BdmvError> {
        if bits == 0 {
            return Ok(0);
        }
        if bits > 32 {
            return Err(BdmvError("bit field too wide".to_string()));
        }

        let mut value = 0u64;
        let mut remaining = bits;

        while remaining > 0 {
            let byte_index = self.bit / 8;
            let byte = *self
                .data
                .get(byte_index)
                .ok_or_else(|| BdmvError("bit field truncated".to_string()))?;

            let bit_in_byte = self.bit % 8;
            let free = 8 - bit_in_byte;
            let take = if remaining < free { remaining } else { free };
            let shift = free - take;
            let mask = (1u64 << take) - 1;
            let chunk = u64::from(byte >> shift) & mask;

            value = (value << take) | chunk;
            self.bit = self
                .bit
                .checked_add(take)
                .ok_or_else(|| BdmvError("bit position overflow".to_string()))?;
            remaining -= take;
        }

        Ok(value)
    }

    fn skip(&mut self, bits: usize) -> Result<(), BdmvError> {
        self.read(bits).map(|_| ())
    }
}

fn write_header(buf: &mut Vec<u8>) -> Result<HeaderPositions, BdmvError> {
    buf.extend_from_slice(b"HDMV");
    buf.extend_from_slice(VERSION);

    let seq = buf.len();
    buf.extend_from_slice(&[0; 4]);
    let prog = buf.len();
    buf.extend_from_slice(&[0; 4]);
    let cpi = buf.len();
    buf.extend_from_slice(&[0; 4]);
    let mark = buf.len();
    buf.extend_from_slice(&[0; 4]);

    buf.extend_from_slice(&[0; 4]);
    buf.extend_from_slice(&[0; 12]);

    Ok(HeaderPositions {
        seq,
        prog,
        cpi,
        mark,
    })
}

fn write_clip_info(buf: &mut Vec<u8>, clip: &ClipInfo) -> Result<(), BdmvError> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0; 2]);
    body.push(1);
    body.push(1);
    body.extend_from_slice(&[0; 4]);
    body.extend_from_slice(&clip.ts_recording_rate.to_be_bytes());
    body.extend_from_slice(&clip.source_packets.to_be_bytes());
    body.extend_from_slice(&[0; 128]);
    write_ts_type_info(&mut body)?;

    let len = len_u32(body.len())?;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&body);
    Ok(())
}

fn write_ts_type_info(buf: &mut Vec<u8>) -> Result<(), BdmvError> {
    buf.push(0x80);
    buf.push(0);
    buf.extend_from_slice(b"HDMV");
    buf.extend_from_slice(&[0; 26]);
    Ok(())
}

fn write_sequence_info(buf: &mut Vec<u8>, clip: &ClipInfo) -> Result<(), BdmvError> {
    let mut body = Vec::new();
    body.push(0);
    body.push(1);
    body.extend_from_slice(&0u32.to_be_bytes());
    body.push(1);
    body.push(0);
    body.extend_from_slice(&clip.pcr_pid.to_be_bytes());
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&clip.presentation_start.to_be_bytes());
    body.extend_from_slice(&clip.presentation_end.to_be_bytes());

    let len = len_u32(body.len())?;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&body);
    Ok(())
}

fn write_program_info(buf: &mut Vec<u8>, clip: &ClipInfo) -> Result<(), BdmvError> {
    let mut body = Vec::new();
    body.push(0);
    body.push(1);
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&clip.pmt_pid.to_be_bytes());

    let n_streams = u8::try_from(clip.streams.len())
        .map_err(|_| BdmvError("too many program streams".to_string()))?;
    body.push(n_streams);
    body.push(0);

    for stream in &clip.streams {
        body.extend_from_slice(&stream.pid.to_be_bytes());
        write_stream_coding_info(&mut body, &stream.coding)?;
    }

    let len = len_u32(body.len())?;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&body);
    Ok(())
}

fn write_stream_coding_info(buf: &mut Vec<u8>, coding: &StreamCoding) -> Result<(), BdmvError> {
    let mut body = Vec::new();
    body.push(coding.coding_type());

    match coding {
        StreamCoding::Mpeg2Video {
            video_format,
            frame_rate,
            aspect,
        }
        | StreamCoding::H264Video {
            video_format,
            frame_rate,
            aspect,
        } => {
            body.push(pack_nibble_pair(*video_format, *frame_rate)?);
            body.push(pack_high_nibble(*aspect)?);
            body.extend_from_slice(&[0; 2]);
        }
        StreamCoding::Lpcm {
            audio_format,
            sample_rate,
            language,
        }
        | StreamCoding::Ac3 {
            audio_format,
            sample_rate,
            language,
        } => {
            body.push(pack_nibble_pair(*audio_format, *sample_rate)?);
            body.extend_from_slice(language);
        }
        StreamCoding::Pgs { language } | StreamCoding::Igs { language } => {
            body.extend_from_slice(language);
        }
    }

    let len = u8::try_from(body.len())
        .map_err(|_| BdmvError("stream coding info too large".to_string()))?;
    buf.push(len);
    buf.extend_from_slice(&body);
    Ok(())
}

fn write_cpi(buf: &mut Vec<u8>, clip: &ClipInfo) -> Result<(), BdmvError> {
    let fine_count = u32::try_from(clip.ep_map.len())
        .map_err(|_| BdmvError("too many EP fine entries".to_string()))?;
    if fine_count > 0x3FFFF {
        return Err(BdmvError("too many EP fine entries".to_string()));
    }

    let groups = build_coarse_groups(&clip.ep_map)?;
    let coarse_count = u16::try_from(groups.len())
        .map_err(|_| BdmvError("too many EP coarse entries".to_string()))?;

    let mut body = Vec::new();
    body.extend_from_slice(&[0, 1, 0, 1]);

    let ep_map_start = 2usize;
    let entry_start = body.len();
    let block_start = entry_start
        .checked_add(12)
        .ok_or_else(|| BdmvError("EP map offset overflow".to_string()))?;
    let epmap_addr = u32::try_from(
        block_start
            .checked_sub(ep_map_start)
            .ok_or_else(|| BdmvError("EP map offset underflow".to_string()))?,
    )
    .map_err(|_| BdmvError("EP map offset too large".to_string()))?;

    write_stream_pid_entry(&mut body, clip.ep_pid, coarse_count, fine_count, epmap_addr)?;

    let block_start = body.len();
    body.extend_from_slice(&[0; 4]);
    write_coarse_entries(&mut body, &groups)?;

    let fine_table_offset = fine_table_offset(groups.len())?;
    patch_u32(&mut body, block_start, fine_table_offset)?;

    write_fine_entries(&mut body, &clip.ep_map)?;

    let len = len_u32(body.len())?;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&body);
    Ok(())
}

fn write_stream_pid_entry(
    body: &mut Vec<u8>,
    pid: u16,
    coarse_count: u16,
    fine_count: u32,
    epmap_addr: u32,
) -> Result<(), BdmvError> {
    body.extend_from_slice(&pid.to_be_bytes());

    let mut bw = BitWriter::new();
    bw.write(0, 10)?;
    bw.write(1, 4)?;
    bw.write(u64::from(coarse_count), 16)?;
    bw.write(u64::from(fine_count), 18)?;
    bw.write(u64::from(epmap_addr), 32)?;

    body.extend_from_slice(&bw.finish());
    Ok(())
}

fn write_coarse_entries(buf: &mut Vec<u8>, groups: &[CoarseGroup]) -> Result<(), BdmvError> {
    for group in groups {
        let mut bw = BitWriter::new();
        bw.write(u64::from(group.first_fine_index), 18)?;
        bw.write(u64::from(group.pts_coarse), 14)?;
        bw.write(u64::from(group.spn_coarse), 32)?;
        buf.extend_from_slice(&bw.finish());
    }
    Ok(())
}

fn write_fine_entries(buf: &mut Vec<u8>, entries: &[EpEntry]) -> Result<(), BdmvError> {
    for entry in entries {
        let pts_fine = u32::try_from((entry.pts >> 9) & 0x7FF)
            .map_err(|_| BdmvError("EP fine PTS overflow".to_string()))?;
        let spn_fine = entry.spn & 0x1FFFF;

        let mut bw = BitWriter::new();
        bw.write(0, 1)?;
        bw.write(1, 3)?;
        bw.write(u64::from(pts_fine), 11)?;
        bw.write(u64::from(spn_fine), 17)?;
        buf.extend_from_slice(&bw.finish());
    }
    Ok(())
}

fn write_clip_mark(buf: &mut Vec<u8>) -> Result<(), BdmvError> {
    buf.extend_from_slice(&0u32.to_be_bytes());
    Ok(())
}

fn build_coarse_groups(entries: &[EpEntry]) -> Result<Vec<CoarseGroup>, BdmvError> {
    let mut groups: Vec<CoarseGroup> = Vec::new();

    for (index, entry) in entries.iter().enumerate() {
        let fine_index =
            u32::try_from(index).map_err(|_| BdmvError("too many EP fine entries".to_string()))?;
        let pts_coarse = u32::try_from((entry.pts >> 19) & 0x3FFF)
            .map_err(|_| BdmvError("EP coarse PTS overflow".to_string()))?;
        let spn_high = entry.spn >> 17;

        let needs_new = match groups.last() {
            None => true,
            Some(g) => g.pts_coarse != pts_coarse || (g.spn_coarse >> 17) != spn_high,
        };

        if needs_new {
            groups.push(CoarseGroup {
                first_fine_index: fine_index,
                pts_coarse,
                spn_coarse: entry.spn,
            });
        }
    }

    Ok(groups)
}

fn fine_table_offset(coarse_count: usize) -> Result<u32, BdmvError> {
    let offset = coarse_count
        .checked_mul(8)
        .and_then(|v| v.checked_add(4))
        .ok_or_else(|| BdmvError("EP table offset overflow".to_string()))?;
    u32::try_from(offset).map_err(|_| BdmvError("EP table offset too large".to_string()))
}

fn read_clip_info(r: &mut Reader<'_>) -> Result<(u32, u32), BdmvError> {
    let _len = r.u32()?;
    r.skip(2)?;
    let _clip_stream_type = r.u8()?;
    let _application_type = r.u8()?;
    r.skip(4)?;

    let ts_recording_rate = r.u32()?;
    let source_packets = r.u32()?;
    r.skip(128)?;
    r.skip(32)?;

    Ok((ts_recording_rate, source_packets))
}

fn read_sequence_info(data: &[u8], addr: u32) -> Result<(u16, u32, u32), BdmvError> {
    let mut r = Reader::at(data, usize_from_u32(addr)?);
    let _len = r.u32()?;
    r.skip(1)?;

    let n_atc = r.u8()?;
    if n_atc != 1 {
        return Err(BdmvError("only one ATC sequence is supported".to_string()));
    }

    let _spn_atc = r.u32()?;
    let n_stc = r.u8()?;
    if n_stc != 1 {
        return Err(BdmvError("only one STC sequence is supported".to_string()));
    }

    r.skip(1)?;
    let pcr_pid = r.u16()?;
    let _spn_stc = r.u32()?;
    let presentation_start = r.u32()?;
    let presentation_end = r.u32()?;

    Ok((pcr_pid, presentation_start, presentation_end))
}

fn read_program_info(data: &[u8], addr: u32) -> Result<(u16, Vec<Stream>), BdmvError> {
    let mut r = Reader::at(data, usize_from_u32(addr)?);
    let _len = r.u32()?;
    r.skip(1)?;

    let n_programs = r.u8()?;
    if n_programs != 1 {
        return Err(BdmvError("only one program is supported".to_string()));
    }

    let _spn_program = r.u32()?;
    let pmt_pid = r.u16()?;
    let n_streams = r.u8()?;
    r.skip(1)?;

    let mut streams = Vec::new();
    for _ in 0..u32::from(n_streams) {
        let pid = r.u16()?;
        let coding = read_stream_coding_info(&mut r)?;
        streams.push(Stream { pid, coding });
    }

    Ok((pmt_pid, streams))
}

fn read_stream_coding_info(r: &mut Reader<'_>) -> Result<StreamCoding, BdmvError> {
    let len = usize::from(r.u8()?);
    if len == 0 {
        return Err(BdmvError("empty stream coding info".to_string()));
    }

    let coding_type = r.u8()?;
    let (coding, consumed) = match coding_type {
        0x02 => read_video_coding_info(r, true)?,
        0x1B => read_video_coding_info(r, false)?,
        0x80 => read_audio_coding_info(r, true)?,
        0x81 => read_audio_coding_info(r, false)?,
        0x90 => read_lang_coding_info(r, true)?,
        0x91 => read_lang_coding_info(r, false)?,
        _ => return Err(BdmvError("unsupported stream coding type".to_string())),
    };

    let remaining = len
        .checked_sub(consumed)
        .ok_or_else(|| BdmvError("stream coding info truncated".to_string()))?;
    r.skip(remaining)?;

    Ok(coding)
}

fn read_video_coding_info(
    r: &mut Reader<'_>,
    mpeg2: bool,
) -> Result<(StreamCoding, usize), BdmvError> {
    let b0 = r.u8()?;
    let b1 = r.u8()?;
    r.skip(2)?;

    let video_format = b0 >> 4;
    let frame_rate = b0 & 0x0F;
    let aspect = b1 >> 4;

    let coding = if mpeg2 {
        StreamCoding::Mpeg2Video {
            video_format,
            frame_rate,
            aspect,
        }
    } else {
        StreamCoding::H264Video {
            video_format,
            frame_rate,
            aspect,
        }
    };

    Ok((coding, 5))
}

fn read_audio_coding_info(
    r: &mut Reader<'_>,
    lpcm: bool,
) -> Result<(StreamCoding, usize), BdmvError> {
    let b = r.u8()?;
    let language = read_lang(r)?;

    let audio_format = b >> 4;
    let sample_rate = b & 0x0F;

    let coding = if lpcm {
        StreamCoding::Lpcm {
            audio_format,
            sample_rate,
            language,
        }
    } else {
        StreamCoding::Ac3 {
            audio_format,
            sample_rate,
            language,
        }
    };

    Ok((coding, 5))
}

fn read_lang_coding_info(
    r: &mut Reader<'_>,
    pgs: bool,
) -> Result<(StreamCoding, usize), BdmvError> {
    let language = read_lang(r)?;
    let coding = if pgs {
        StreamCoding::Pgs { language }
    } else {
        StreamCoding::Igs { language }
    };
    Ok((coding, 4))
}

fn read_cpi(data: &[u8], cpi_addr: u32) -> Result<(u16, Vec<EpEntry>), BdmvError> {
    let cpi_pos = usize_from_u32(cpi_addr)?;
    let mut r = Reader::at(data, cpi_pos);
    let len = r.u32()?;

    if len == 0 {
        return Ok((0, Vec::new()));
    }

    let _b0 = r.u8()?;
    let b1 = r.u8()?;
    let _b2 = r.u8()?;
    let count = r.u8()?;

    if (b1 & 0x0F) != 1 {
        return Err(BdmvError("unsupported CPI type".to_string()));
    }
    if count != 1 {
        return Err(BdmvError(
            "only one EP stream PID entry is supported".to_string(),
        ));
    }

    let entry = read_stream_pid_entry(&mut r)?;
    let ep_map_start = cpi_pos
        .checked_add(6)
        .ok_or_else(|| BdmvError("EP map start overflow".to_string()))?;
    let block_offset = ep_map_start
        .checked_add(usize_from_u32(entry.epmap_addr)?)
        .ok_or_else(|| BdmvError("EP map block overflow".to_string()))?;

    let fine_table_addr = Reader::at(data, block_offset).u32()?;
    let coarse_start = block_offset
        .checked_add(4)
        .ok_or_else(|| BdmvError("EP coarse table overflow".to_string()))?;
    let fine_start = block_offset
        .checked_add(usize_from_u32(fine_table_addr)?)
        .ok_or_else(|| BdmvError("EP fine table overflow".to_string()))?;

    let coarse = read_coarse_entries(data, coarse_start, entry.coarse_count)?;
    let ep_map = read_fine_entries(data, fine_start, entry.fine_count, &coarse)?;

    Ok((entry.ep_pid, ep_map))
}

fn read_stream_pid_entry(r: &mut Reader<'_>) -> Result<StreamPidEntry, BdmvError> {
    let bytes = r.bytes(12)?;
    let mut br = BitReader::at(bytes);

    let ep_pid = u16_from_bits(br.read(16)?)?;
    br.skip(10)?;

    let ep_stream_type = u32_from_bits(br.read(4)?)?;
    if ep_stream_type != 1 {
        return Err(BdmvError("unsupported EP stream type".to_string()));
    }

    let coarse_count = u16_from_bits(br.read(16)?)?;
    let fine_count = u32_from_bits(br.read(18)?)?;
    let epmap_addr = u32_from_bits(br.read(32)?)?;

    Ok(StreamPidEntry {
        ep_pid,
        coarse_count,
        fine_count,
        epmap_addr,
    })
}

fn read_coarse_entries(data: &[u8], start: usize, count: u16) -> Result<Vec<Coarse>, BdmvError> {
    let mut r = Reader::at(data, start);
    let mut out = Vec::new();

    for _ in 0..u32::from(count) {
        let bytes = r.bytes(8)?;
        let mut br = BitReader::at(bytes);

        let ref_to_fine = u32_from_bits(br.read(18)?)?;
        let pts_coarse = u32_from_bits(br.read(14)?)?;
        let spn_coarse = u32_from_bits(br.read(32)?)?;

        out.push(Coarse {
            ref_to_fine,
            pts_coarse,
            spn_coarse,
        });
    }

    Ok(out)
}

fn read_fine_entries(
    data: &[u8],
    start: usize,
    count: u32,
    coarse: &[Coarse],
) -> Result<Vec<EpEntry>, BdmvError> {
    let mut r = Reader::at(data, start);
    let mut out = Vec::new();
    let mut idx = 0usize;

    for fine_idx in 0..count {
        idx = advance_coarse(coarse, idx, fine_idx)?;
        let c = coarse
            .get(idx)
            .ok_or_else(|| BdmvError("missing EP coarse entry".to_string()))?;

        let bytes = r.bytes(4)?;
        let mut br = BitReader::at(bytes);
        br.skip(1)?;
        br.skip(3)?;

        let pts_fine = u32_from_bits(br.read(11)?)?;
        let spn_fine = u32_from_bits(br.read(17)?)?;

        let pts = (u64::from(c.pts_coarse) << 19) | (u64::from(pts_fine) << 9);
        let spn = (c.spn_coarse & !0x1FFFFu32) | spn_fine;

        out.push(EpEntry { pts, spn });
    }

    Ok(out)
}

fn advance_coarse(coarse: &[Coarse], mut idx: usize, fine_idx: u32) -> Result<usize, BdmvError> {
    while let Some(next_idx) = idx.checked_add(1) {
        match coarse.get(next_idx) {
            Some(next) if next.ref_to_fine <= fine_idx => idx = next_idx,
            _ => break,
        }
    }
    Ok(idx)
}

fn read_lang(r: &mut Reader<'_>) -> Result<[u8; 3], BdmvError> {
    r.bytes(3)?
        .try_into()
        .map_err(|_| BdmvError("language code truncated".to_string()))
}

fn pack_nibble_pair(high: u8, low: u8) -> Result<u8, BdmvError> {
    let value = (u32::from(high) << 4) | u32::from(low);
    u8::try_from(value).map_err(|_| BdmvError("4-bit field overflow".to_string()))
}

fn pack_high_nibble(value: u8) -> Result<u8, BdmvError> {
    pack_nibble_pair(value, 0)
}

fn usize_from_u32(value: u32) -> Result<usize, BdmvError> {
    usize::try_from(value).map_err(|_| BdmvError("address out of range".to_string()))
}

fn u16_from_bits(value: u64) -> Result<u16, BdmvError> {
    u16::try_from(value).map_err(|_| BdmvError("bit field out of range".to_string()))
}

fn u32_from_bits(value: u64) -> Result<u32, BdmvError> {
    u32::try_from(value).map_err(|_| BdmvError("bit field out of range".to_string()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::{Stream, StreamCoding};

    fn sample_clip() -> ClipInfo {
        let streams = vec![
            Stream {
                pid: 0x1011,
                coding: StreamCoding::H264Video {
                    video_format: 6,
                    frame_rate: 4,
                    aspect: 3,
                },
            },
            Stream {
                pid: 0x1100,
                coding: StreamCoding::Ac3 {
                    audio_format: 3,
                    sample_rate: 1,
                    language: *b"jpn",
                },
            },
            Stream {
                pid: 0x1200,
                coding: StreamCoding::Pgs { language: *b"eng" },
            },
        ];

        let ep_map = vec![
            EpEntry { pts: 0, spn: 0 },
            EpEntry { pts: 512, spn: 1 },
            EpEntry { pts: 1024, spn: 2 },
            EpEntry {
                pts: 1u64 << 19,
                spn: 3,
            },
            EpEntry {
                pts: (1u64 << 19) + 512,
                spn: 4,
            },
        ];

        ClipInfo {
            ts_recording_rate: 48_000_000,
            source_packets: 1000,
            pcr_pid: 0x1000,
            pmt_pid: 0x1010,
            presentation_start: 0,
            presentation_end: 45_000,
            streams,
            ep_pid: 0x1011,
            ep_map,
        }
    }

    #[test]
    fn round_trip() {
        let clip = sample_clip();
        let bytes = write_clpi(&clip).unwrap();
        assert_eq!(read_clpi(&bytes).unwrap(), clip);
    }

    #[test]
    fn header() {
        let bytes = write_clpi(&sample_clip()).unwrap();
        assert_eq!(&bytes[0..8], b"HDMV0200");
    }

    #[test]
    fn coarse_groups_cross_boundary() {
        let clip = sample_clip();
        let groups = build_coarse_groups(&clip.ep_map).unwrap();
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn fine_bit_packing() {
        let entry = EpEntry { pts: 0x400, spn: 3 };
        let mut out = Vec::new();
        write_fine_entries(&mut out, &[entry]).unwrap();
        assert_eq!(out, vec![0x10, 0x04, 0x00, 0x03]);
    }

    #[test]
    fn truncated_input_is_error() {
        let bytes = write_clpi(&sample_clip()).unwrap();
        assert!(read_clpi(&bytes[..bytes.len() - 1]).is_err());
    }
}
