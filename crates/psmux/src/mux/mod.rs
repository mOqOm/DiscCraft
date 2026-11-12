//! DVD VOBS muxer for milestone M2.

use crate::es::{Ac3Stream, LpcmStream, PictureType, VideoStream, to_90k};
use crate::sink::SectorSink;
use std::fmt;
use std::io;

pub mod hli;
pub mod menu;
pub mod nav;
pub mod pack;
mod schedule;

/// Muxer error.
#[derive(Debug)]
pub enum MuxError {
    /// Underlying I/O error.
    Io(io::Error),
    /// Invalid input or impossible muxing condition.
    Invalid(String),
}

impl fmt::Display for MuxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Invalid(msg) => write!(f, "invalid input: {msg}"),
        }
    }
}

impl std::error::Error for MuxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Invalid(_) => None,
        }
    }
}

impl From<io::Error> for MuxError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// One audio source. The vector index is the DVD audio stream number.
#[derive(Debug)]
pub enum AudioSource<'a> {
    /// AC-3 stream and its elementary-stream bytes.
    Ac3 {
        /// Parsed AC-3 stream.
        stream: &'a Ac3Stream,
        /// AC-3 elementary stream bytes.
        data: &'a [u8],
    },
    /// DVD LPCM stream parsed from WAV.
    Lpcm(&'a LpcmStream),
}

/// One complete DVD subpicture unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubpictureUnit {
    /// Presentation timestamp, 27 MHz, absolute like video PTS.
    pub pts: i64,
    /// Complete SPU bytes.
    pub data: Vec<u8>,
}

/// Maximum number of DVD subpicture streams.
pub(crate) const SUBPICTURE_COUNT: usize = 32;

/// Input for one title VOBS.
#[derive(Debug)]
pub struct MuxInput<'a> {
    /// Parsed MPEG-2 video stream.
    pub video: &'a VideoStream,
    /// Video elementary stream bytes.
    pub video_data: &'a [u8],
    /// Audio streams, index = DVD audio stream number, max 8.
    pub audio: Vec<AudioSource<'a>>,
    /// Audio start PTS in 27 MHz units.
    pub audio_start_pts: i64,
    /// Subpicture streams, index = DVD subpicture stream number
    /// (sub stream id `0x20 + n`), max 32; units sorted by pts.
    pub subpictures: Vec<Vec<SubpictureUnit>>,
    /// Requested cell start times, 27 MHz presentation time relative to first video frame.
    pub cell_starts: Vec<i64>,
    /// VOB id written into NAV packs.
    pub vob_id: u16,
    /// True for NTSC, false for PAL.
    pub ntsc: bool,
}

/// Public information about one VOBU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VobuInfo {
    /// Sector index of the VOBU NAV pack.
    pub start_sector: u32,
    /// Sector index of the last pack in the VOBU.
    pub last_sector: u32,
    /// VOBU start presentation time, 90 kHz.
    pub s_ptm: u32,
    /// VOBU end presentation time, 90 kHz.
    pub e_ptm: u32,
    /// Zero-based cell index.
    pub cell: usize,
}

/// Public layout of one cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellLayout {
    /// One-based cell id.
    pub cell_id: u8,
    /// First sector of the cell.
    pub first_sector: u32,
    /// Start sector of the last VOBU in the cell.
    pub last_vobu_start: u32,
    /// Last sector of the cell.
    pub last_sector: u32,
    /// Cell start presentation time, 90 kHz.
    pub start_ptm: u32,
    /// Cell end presentation time, 90 kHz.
    pub end_ptm: u32,
}

/// Complete VOBS layout returned by the muxer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VobsLayout {
    /// VOBUs in output order.
    pub vobus: Vec<VobuInfo>,
    /// Cells in output order.
    pub cells: Vec<CellLayout>,
    /// Total sectors written.
    pub total_sectors: u32,
    /// First video presentation time, 90 kHz.
    pub video_start_ptm: u32,
    /// Last video presentation time, 90 kHz.
    pub video_end_ptm: u32,
}

pub(crate) struct GopInfo {
    pub(crate) start_pic: usize,
    pub(crate) byte_start: usize,
    pub(crate) byte_len: usize,
    pub(crate) display_start: i64,
    pub(crate) display_end: i64,
    pub(crate) first_dts: i64,
    pub(crate) closed: bool,
    pub(crate) pic_starts: Vec<usize>,
    pub(crate) ref_pics: [Option<usize>; 3],
}

pub(crate) struct VobuRecord {
    pub(crate) info: VobuInfo,
    pub(crate) nav_scr: i128,
    pub(crate) ref_last: [Option<u32>; 3],
    pub(crate) first_audio: [Option<u32>; 8],
    pub(crate) first_subpicture: [Option<u32>; SUBPICTURE_COUNT],
    pub(crate) cell_start_ptm: u32,
}

pub(crate) struct CellInfo {
    pub(crate) start_gop: usize,
    pub(crate) end_gop: usize,
    pub(crate) start_vobu: usize,
    pub(crate) end_vobu: usize,
    pub(crate) start_ptm: u32,
}

const GOP_MAX_DURATION: i64 = 27_000_000;

/// Mux one title VOBS into `sink`.
/// Options for VOBS that are not titles (motion menus).
#[derive(Debug, Clone, Copy, Default)]
pub struct MuxExtra<'a> {
    /// Highlight information written into every VOBU's PCI (menu buttons); its start PTM is set per VOBU.
    pub highlight: Option<&'a hli::Hli>,
    /// Sector at which this VOBS starts in `sink` (several menu VOBs share one VIDEO_TS.VOB); NAV LBNs and the layout are absolute.
    pub base_sector: u32,
}

/// Multiplex `input` into `sink`. Equivalent to [`mux_vobs_with`] with default [`MuxExtra`].
pub fn mux_vobs(input: &MuxInput<'_>, sink: &mut dyn SectorSink) -> Result<VobsLayout, MuxError> {
    mux_vobs_with(input, &MuxExtra::default(), sink)
}

/// Multiplex `input` into `sink` with menu options (see [`MuxExtra`]).
pub fn mux_vobs_with(
    input: &MuxInput<'_>,
    extra: &MuxExtra<'_>,
    sink: &mut dyn SectorSink,
) -> Result<VobsLayout, MuxError> {
    if input.audio.len() > 8 {
        return Err(invalid("too many audio streams"));
    }
    if input.subpictures.len() > SUBPICTURE_COUNT {
        return Err(invalid("too many subpicture streams"));
    }

    let gops = build_gops(input.video, input.video_data)?;
    let mut cells = build_cells(input, &gops)?;
    let mut audio_states = schedule::init_audio_states(input)?;
    let mut subpicture_states = schedule::init_subpicture_states(input)?;

    let mut records: Vec<VobuRecord> = Vec::new();
    let mut total_sectors = extra.base_sector;
    let mut prev_scr = 0i128;
    let mut rem = 0i128;

    for (cell_idx, cell) in cells.iter().enumerate() {
        let cell_id = u8::try_from(cell_idx + 1).map_err(|_| invalid("too many cells"))?;
        for gop_idx in cell.start_gop..cell.end_gop {
            let next_gop_dts = gops.get(gop_idx + 1).map(|next| next.first_dts);
            let append_seq_end = gop_idx + 1 == gops.len() && !input.video.has_sequence_end;
            let (record, new_total, new_prev, new_rem) = schedule::mux_vobu(
                input,
                &gops,
                gop_idx,
                cell_idx,
                cell_id,
                cell.start_ptm,
                next_gop_dts,
                append_seq_end,
                &mut audio_states,
                &mut subpicture_states,
                sink,
                total_sectors,
                prev_scr,
                rem,
                records.is_empty(),
            )?;
            records.push(record);
            total_sectors = new_total;
            prev_scr = new_prev;
            rem = new_rem;
        }
    }

    update_cell_vobu_ranges(&mut cells, &records)?;
    let layout = build_layout(&records, &cells, total_sectors)?;

    sink.finish()?;
    nav::backfill(input, &records, &cells, extra.highlight, sink)?;
    sink.finish()?;
    Ok(layout)
}

pub(crate) fn invalid(msg: &str) -> MuxError {
    MuxError::Invalid(msg.to_string())
}

pub(crate) fn u32_from_i64(value: i64) -> Result<u32, MuxError> {
    if value < 0 || value > i64::from(u32::MAX) {
        return Err(invalid("timestamp out of range"));
    }
    Ok(value as u32)
}

fn build_gops(video: &VideoStream, data: &[u8]) -> Result<Vec<GopInfo>, MuxError> {
    if video.pictures.is_empty() {
        return Err(invalid("no pictures"));
    }

    let mut gops = Vec::new();
    let mut start = 0usize;
    let len = video.pictures.len();

    for i in 1..=len {
        let boundary = if i == len {
            true
        } else {
            match video.pictures.get(i) {
                Some(pic) => pic.gop_start,
                None => return Err(invalid("missing picture")),
            }
        };
        if boundary {
            gops.push(build_gop(video, data, start, i)?);
            start = i;
        }
    }

    Ok(gops)
}

fn build_gop(
    video: &VideoStream,
    data: &[u8],
    start: usize,
    end: usize,
) -> Result<GopInfo, MuxError> {
    let first = video
        .pictures
        .get(start)
        .ok_or_else(|| invalid("missing picture"))?;
    if !first.gop_start {
        return Err(invalid("GOP must start with a GOP header"));
    }
    if first.kind != PictureType::I {
        return Err(invalid("GOP must start with an I picture"));
    }
    if !first.has_sequence_header {
        return Err(invalid("every GOP must start with a sequence header"));
    }

    let last = video
        .pictures
        .get(end - 1)
        .ok_or_else(|| invalid("missing picture"))?;
    let byte_start = first.offset;
    let byte_end = last
        .offset
        .checked_add(last.len)
        .ok_or_else(|| invalid("picture length overflow"))?;
    if byte_end > data.len() {
        return Err(invalid("picture data out of range"));
    }
    let byte_len = byte_end
        .checked_sub(byte_start)
        .ok_or_else(|| invalid("picture offsets out of order"))?;

    let mut display_start = i64::MAX;
    let mut display_end = i64::MIN;
    let mut pic_starts = Vec::new();
    let mut ref_pics = [None, None, None];
    let mut ref_count = 0usize;

    for idx in start..end {
        let pic = video
            .pictures
            .get(idx)
            .ok_or_else(|| invalid("missing picture"))?;
        let rel = pic
            .offset
            .checked_sub(byte_start)
            .ok_or_else(|| invalid("picture offset before GOP"))?;
        if let Some(&prev) = pic_starts.last()
            && rel < prev
        {
            return Err(invalid("picture offsets out of order"));
        }
        pic_starts.push(rel);

        if pic.pts < display_start {
            display_start = pic.pts;
        }
        let pic_end = pic
            .pts
            .checked_add(pic.duration)
            .ok_or_else(|| invalid("display end overflow"))?;
        if pic_end > display_end {
            display_end = pic_end;
        }

        if pic.kind == PictureType::I || pic.kind == PictureType::P {
            if let Some(slot) = ref_pics.get_mut(ref_count) {
                *slot = Some(idx);
            }
            ref_count = ref_count.saturating_add(1);
        }
    }

    if display_start == i64::MAX || display_end == i64::MIN {
        return Err(invalid("empty GOP"));
    }
    let duration = display_end
        .checked_sub(display_start)
        .ok_or_else(|| invalid("GOP duration overflow"))?;
    if !(0..=GOP_MAX_DURATION).contains(&duration) {
        return Err(invalid("GOP longer than 1 s"));
    }

    Ok(GopInfo {
        start_pic: start,
        byte_start,
        byte_len,
        display_start,
        display_end,
        first_dts: first.dts,
        closed: first.closed_gop,
        pic_starts,
        ref_pics,
    })
}

fn build_cells(input: &MuxInput<'_>, gops: &[GopInfo]) -> Result<Vec<CellInfo>, MuxError> {
    if input.cell_starts.is_empty() {
        return Err(invalid("cell_starts must not be empty"));
    }
    if input.cell_starts.first().copied() != Some(0) {
        return Err(invalid("first cell start must be 0"));
    }
    if input.cell_starts.len() > 255 {
        return Err(invalid("too many cells"));
    }

    let mut prev_time: Option<i64> = None;
    for &t in input.cell_starts.iter() {
        if let Some(prev) = prev_time
            && prev >= t
        {
            return Err(invalid("cell starts must be strictly increasing"));
        }
        prev_time = Some(t);
    }

    let first_pts = gops
        .first()
        .ok_or_else(|| invalid("no GOPs"))?
        .display_start;
    let mut chosen = Vec::new();
    let mut prev_gop = 0usize;

    for (idx, &t) in input.cell_starts.iter().enumerate() {
        let target = first_pts
            .checked_add(t)
            .ok_or_else(|| invalid("cell start overflow"))?;
        let best = nearest_gop(gops, target)?;

        if idx == 0 {
            if best != 0 {
                return Err(invalid("first cell must start at GOP 0"));
            }
        } else if best <= prev_gop {
            return Err(invalid(&format!(
                "chapters {} and {} fall in the same GOP",
                idx,
                idx + 1
            )));
        }

        let gop = gops.get(best).ok_or_else(|| invalid("missing GOP"))?;
        if !gop.closed {
            return Err(invalid("first GOP of cell must be closed"));
        }

        chosen.push(best);
        prev_gop = best;
    }

    let mut cells = Vec::new();
    for i in 0..chosen.len() {
        let start_gop = chosen
            .get(i)
            .copied()
            .ok_or_else(|| invalid("missing cell"))?;
        let end_gop = if i + 1 < chosen.len() {
            chosen
                .get(i + 1)
                .copied()
                .ok_or_else(|| invalid("missing cell"))?
        } else {
            gops.len()
        };
        if start_gop >= end_gop {
            return Err(invalid("empty cell"));
        }
        let start_ptm = u32_from_i64(to_90k(
            gops.get(start_gop)
                .ok_or_else(|| invalid("missing GOP"))?
                .display_start,
        ))?;
        cells.push(CellInfo {
            start_gop,
            end_gop,
            start_vobu: usize::MAX,
            end_vobu: 0,
            start_ptm,
        });
    }

    Ok(cells)
}

fn nearest_gop(gops: &[GopInfo], target: i64) -> Result<usize, MuxError> {
    let mut best = 0usize;
    let mut best_dist = i64::MAX;

    for (idx, gop) in gops.iter().enumerate() {
        let dist = abs_diff_i64(gop.display_start, target)?;
        if dist < best_dist {
            best = idx;
            best_dist = dist;
        }
    }

    Ok(best)
}

fn abs_diff_i64(a: i64, b: i64) -> Result<i64, MuxError> {
    if a >= b {
        a.checked_sub(b)
            .ok_or_else(|| invalid("timestamp overflow"))
    } else {
        b.checked_sub(a)
            .ok_or_else(|| invalid("timestamp overflow"))
    }
}

fn update_cell_vobu_ranges(cells: &mut [CellInfo], records: &[VobuRecord]) -> Result<(), MuxError> {
    for (idx, record) in records.iter().enumerate() {
        let cell = cells
            .get_mut(record.info.cell)
            .ok_or_else(|| invalid("bad cell index"))?;
        if cell.start_vobu == usize::MAX {
            cell.start_vobu = idx;
        }
        cell.end_vobu = idx.saturating_add(1);
    }

    for cell in cells.iter() {
        if cell.start_vobu == usize::MAX {
            return Err(invalid("cell has no VOBUs"));
        }
    }

    Ok(())
}

fn build_layout(
    records: &[VobuRecord],
    cells: &[CellInfo],
    total_sectors: u32,
) -> Result<VobsLayout, MuxError> {
    if records.is_empty() {
        return Err(invalid("no VOBUs"));
    }

    let vobus = records.iter().map(|record| record.info.clone()).collect();
    let mut cell_layouts = Vec::new();

    for (idx, cell) in cells.iter().enumerate() {
        let first = records
            .get(cell.start_vobu)
            .ok_or_else(|| invalid("missing first VOBU"))?;
        let last_index = cell
            .end_vobu
            .checked_sub(1)
            .ok_or_else(|| invalid("empty cell"))?;
        let last = records
            .get(last_index)
            .ok_or_else(|| invalid("missing last VOBU"))?;
        let cell_id = u8::try_from(idx + 1).map_err(|_| invalid("too many cells"))?;

        cell_layouts.push(CellLayout {
            cell_id,
            first_sector: first.info.start_sector,
            last_vobu_start: last.info.start_sector,
            last_sector: last.info.last_sector,
            start_ptm: cell.start_ptm,
            end_ptm: last.info.e_ptm,
        });
    }

    let video_start_ptm = records
        .first()
        .ok_or_else(|| invalid("no VOBUs"))?
        .info
        .s_ptm;
    let video_end_ptm = records
        .last()
        .ok_or_else(|| invalid("no VOBUs"))?
        .info
        .e_ptm;

    Ok(VobsLayout {
        vobus,
        cells: cell_layouts,
        total_sectors,
        video_start_ptm,
        video_end_ptm,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        AudioSource, CellLayout, MuxError, MuxInput, SubpictureUnit, VobsLayout, mux_vobs,
    };
    use crate::es::ac3::parse_ac3;
    use crate::es::mpeg2v::parse_video;
    use crate::es::{LpcmStream, VideoStream};
    use crate::sink::VobFileSink;
    use crate::sink::{MemorySink, SECTOR, SectorSink};
    use std::io::{Read, Seek, SeekFrom};

    const START_DTS: i64 = 8_100_000;
    const FRAME_DURATION: i64 = 900_900;
    const FIRST_PTS: i64 = START_DTS + FRAME_DURATION;
    const GOP2_RELATIVE: i64 = 8 * FRAME_DURATION;
    const PCI_DATA: usize = 45;
    const DSI_DATA: usize = 1031;

    struct BitWriter {
        bytes: Vec<u8>,
        current: u8,
        bits_left: u8,
    }

    impl BitWriter {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                current: 0,
                bits_left: 8,
            }
        }

        fn write_bits(&mut self, value: u32, n: u32) {
            for i in (0..n).rev() {
                let bit = ((value >> i) & 1) as u8;
                self.current |= bit << (self.bits_left - 1);
                self.bits_left -= 1;
                if self.bits_left == 0 {
                    self.bytes.push(self.current);
                    self.current = 0;
                    self.bits_left = 8;
                }
            }
        }

        fn finish(&mut self) {
            if self.bits_left != 8 {
                self.bytes.push(self.current);
                self.current = 0;
                self.bits_left = 8;
            }
        }
    }

    fn append_start_code(bytes: &mut Vec<u8>, code: u8) {
        bytes.extend_from_slice(&[0, 0, 1, code]);
    }

    fn write_sequence_header(bytes: &mut Vec<u8>) {
        append_start_code(bytes, 0xB3);
        let mut bw = BitWriter::new();
        bw.write_bits(720, 12);
        bw.write_bits(480, 12);
        bw.write_bits(3, 4);
        bw.write_bits(4, 4);
        bw.write_bits(10000, 18);
        bw.write_bits(1, 1);
        bw.write_bits(1000, 10);
        bw.write_bits(1, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.finish();
        bytes.extend_from_slice(&bw.bytes);
    }

    fn write_sequence_extension(bytes: &mut Vec<u8>) {
        append_start_code(bytes, 0xB5);
        let mut bw = BitWriter::new();
        bw.write_bits(1, 4);
        bw.write_bits(0x45, 8);
        bw.write_bits(0, 1);
        bw.write_bits(1, 2);
        bw.write_bits(0, 3);
        bw.write_bits(0, 1);
        bw.write_bits(720, 14);
        bw.write_bits(480, 14);
        bw.write_bits(0, 12);
        bw.write_bits(1, 1);
        bw.write_bits(0, 8);
        bw.write_bits(1, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 2);
        bw.write_bits(0, 5);
        bw.finish();
        bytes.extend_from_slice(&bw.bytes);
    }

    fn write_gop(bytes: &mut Vec<u8>, closed: u32) {
        append_start_code(bytes, 0xB8);
        let mut bw = BitWriter::new();
        bw.write_bits(0, 25);
        bw.write_bits(closed, 1);
        bw.write_bits(0, 1);
        bw.finish();
        bytes.extend_from_slice(&bw.bytes);
    }

    fn write_picture_header(bytes: &mut Vec<u8>, tr: u32, coding: u32) {
        append_start_code(bytes, 0x00);
        let mut bw = BitWriter::new();
        bw.write_bits(tr, 10);
        bw.write_bits(coding, 3);
        bw.write_bits(0, 16);
        if coding == 2 || coding == 3 {
            bw.write_bits(0, 1);
            bw.write_bits(0, 3);
        }
        if coding == 3 {
            bw.write_bits(0, 1);
            bw.write_bits(0, 3);
        }
        bw.finish();
        bytes.extend_from_slice(&bw.bytes);
    }

    fn write_picture_coding_extension(bytes: &mut Vec<u8>) {
        append_start_code(bytes, 0xB5);
        let mut bw = BitWriter::new();
        bw.write_bits(8, 4);
        bw.write_bits(0, 16);
        bw.write_bits(0, 2);
        bw.write_bits(3, 2);
        bw.write_bits(1, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.finish();
        bytes.extend_from_slice(&bw.bytes);
    }

    fn write_slice(bytes: &mut Vec<u8>) {
        append_start_code(bytes, 0x01);
        bytes.extend_from_slice(&[0xAA; 3000]);
    }

    fn write_one_picture(bytes: &mut Vec<u8>, tr: u32, coding: u32) {
        write_picture_header(bytes, tr, coding);
        write_picture_coding_extension(bytes);
        write_slice(bytes);
    }

    fn build_test_video() -> Vec<u8> {
        let mut data = Vec::new();
        for _ in 0..3 {
            write_sequence_header(&mut data);
            write_sequence_extension(&mut data);
            write_gop(&mut data, 1);
            write_one_picture(&mut data, 0, 1);
            write_one_picture(&mut data, 3, 2);
            write_one_picture(&mut data, 1, 3);
            write_one_picture(&mut data, 2, 3);
        }
        data
    }

    fn build_ac3_frame() -> Vec<u8> {
        let mut bw = BitWriter::new();
        bw.write_bits(0x0B77, 16);
        bw.write_bits(0, 16);
        bw.write_bits(0, 2);
        bw.write_bits(20, 6);
        bw.write_bits(8, 5);
        bw.write_bits(0, 3);
        bw.write_bits(2, 3);
        bw.write_bits(0, 2);
        bw.write_bits(0, 1);
        bw.finish();

        let mut frame = bw.bytes;
        frame.resize(768, 0);
        frame
    }

    fn build_ac3_data(frames: usize) -> Vec<u8> {
        let mut data = Vec::new();
        for _ in 0..frames {
            data.extend_from_slice(&build_ac3_frame());
        }
        data
    }

    fn parse_video_data(data: &[u8]) -> Result<VideoStream, MuxError> {
        parse_video(data, START_DTS).map_err(|err| MuxError::Invalid(err.to_string()))
    }

    fn parse_ac3_data(data: &[u8], start_pts: i64) -> Result<crate::es::Ac3Stream, MuxError> {
        parse_ac3(data, start_pts).map_err(|err| MuxError::Invalid(err.to_string()))
    }

    fn read_u16(sector: &[u8], offset: usize) -> Option<u16> {
        let end = offset.checked_add(2)?;
        let bytes: [u8; 2] = sector.get(offset..end)?.try_into().ok()?;
        Some(u16::from_be_bytes(bytes))
    }

    fn read_u32(sector: &[u8], offset: usize) -> Option<u32> {
        let end = offset.checked_add(4)?;
        let bytes: [u8; 4] = sector.get(offset..end)?.try_into().ok()?;
        Some(u32::from_be_bytes(bytes))
    }

    fn scr_from_sector(sector: &[u8]) -> Option<i64> {
        let mut bits: u64 = 0;
        for i in 4..10 {
            bits = (bits << 8) | u64::from(*sector.get(i)?);
        }
        let base32 = (bits >> 43) & 0x7;
        let base29 = (bits >> 27) & 0x7FFF;
        let base14 = (bits >> 11) & 0x7FFF;
        let ext = (bits >> 1) & 0x1FF;
        let base = (base32 << 30) | (base29 << 15) | base14;
        i64::try_from(base.saturating_mul(300).saturating_add(ext)).ok()
    }

    #[derive(Debug)]
    struct SubChunk {
        sector: u32,
        pts: Option<i64>,
        data: Vec<u8>,
    }

    fn data_pack_payload(sector: &[u8]) -> Result<Option<(u8, usize, &[u8])>, MuxError> {
        let Some(stream_id) = sector.get(17).copied() else {
            return Ok(None);
        };
        if stream_id != 0xE0 && stream_id != 0xBD {
            return Ok(None);
        }

        let packet_len = read_u16(sector, 18)
            .ok_or_else(|| MuxError::Invalid("missing packet length".to_string()))?
            as usize;
        let hdr_len = sector
            .get(22)
            .copied()
            .ok_or_else(|| MuxError::Invalid("missing header length".to_string()))?
            as usize;
        let base_len = 3usize
            .checked_add(hdr_len)
            .ok_or_else(|| MuxError::Invalid("PES length overflow".to_string()))?;
        if packet_len < base_len {
            return Err(MuxError::Invalid("bad PES packet length".to_string()));
        }
        let payload_len = packet_len - base_len;
        let payload_off = 23usize
            .checked_add(hdr_len)
            .ok_or_else(|| MuxError::Invalid("PES offset overflow".to_string()))?;
        let payload_end = payload_off
            .checked_add(payload_len)
            .ok_or_else(|| MuxError::Invalid("PES payload overflow".to_string()))?;
        let payload = sector
            .get(payload_off..payload_end)
            .ok_or_else(|| MuxError::Invalid("PES payload out of range".to_string()))?;

        Ok(Some((stream_id, hdr_len, payload)))
    }

    fn extract_video_ac3_payloads(sink: &MemorySink) -> Result<(Vec<u8>, Vec<u8>), MuxError> {
        let mut video = Vec::new();
        let mut ac3 = Vec::new();

        for sector in sink.sectors.iter() {
            let Some((stream_id, _hdr_len, payload)) = data_pack_payload(sector)? else {
                continue;
            };

            if stream_id == 0xE0 {
                video.extend_from_slice(payload);
            } else if stream_id == 0xBD {
                let sub = payload
                    .first()
                    .copied()
                    .ok_or_else(|| MuxError::Invalid("missing audio sub stream".to_string()))?;
                if (0x80..=0x87).contains(&sub) {
                    let audio = payload
                        .get(4..)
                        .ok_or_else(|| MuxError::Invalid("missing AC-3 payload".to_string()))?;
                    ac3.extend_from_slice(audio);
                }
            }
        }

        Ok((video, ac3))
    }

    fn read_pts(sector: &[u8], offset: usize) -> Option<i64> {
        let end = offset.checked_add(5)?;
        let bytes: [u8; 5] = sector.get(offset..end)?.try_into().ok()?;
        let bits = (u64::from(bytes[0]) << 32)
            | (u64::from(bytes[1]) << 24)
            | (u64::from(bytes[2]) << 16)
            | (u64::from(bytes[3]) << 8)
            | u64::from(bytes[4]);

        let prefix = (bits >> 36) & 0xF;
        if prefix != 2 {
            return None;
        }

        let base30 = (bits >> 33) & 0x7;
        let base29 = (bits >> 17) & 0x7FFF;
        let base14 = (bits >> 1) & 0x7FFF;
        let ts = (base30 << 30) | (base29 << 15) | base14;
        i64::try_from(ts).ok()
    }

    fn subpicture_pts(sector: &[u8], hdr_len: usize) -> Result<Option<i64>, MuxError> {
        let flags2 = sector
            .get(21)
            .copied()
            .ok_or_else(|| MuxError::Invalid("missing PES flags".to_string()))?;
        if flags2 & 0x80 == 0 {
            return Ok(None);
        }
        if hdr_len < 5 {
            return Err(MuxError::Invalid(
                "subpicture PTS without header".to_string(),
            ));
        }
        read_pts(sector, 23)
            .ok_or_else(|| MuxError::Invalid("bad subpicture PTS".to_string()))
            .map(Some)
    }

    fn subpicture_chunk_from_sector(
        sector: &[u8],
        sector_idx: u32,
        sub_id: u8,
    ) -> Result<Option<SubChunk>, MuxError> {
        let Some((stream_id, hdr_len, payload)) = data_pack_payload(sector)? else {
            return Ok(None);
        };
        if stream_id != 0xBD {
            return Ok(None);
        }
        if payload.first().copied() != Some(sub_id) {
            return Ok(None);
        }

        let pts = subpicture_pts(sector, hdr_len)?;
        let data = payload
            .get(1..)
            .ok_or_else(|| MuxError::Invalid("missing subpicture data".to_string()))?
            .to_vec();

        Ok(Some(SubChunk {
            sector: sector_idx,
            pts,
            data,
        }))
    }

    fn extract_subpicture_chunks(sink: &MemorySink, sub_id: u8) -> Result<Vec<SubChunk>, MuxError> {
        let mut out = Vec::new();

        for (idx, sector) in sink.sectors.iter().enumerate() {
            let sector_idx = u32::try_from(idx)
                .map_err(|_| MuxError::Invalid("sector index overflow".to_string()))?;
            if let Some(chunk) = subpicture_chunk_from_sector(sector, sector_idx, sub_id)? {
                out.push(chunk);
            }
        }

        Ok(out)
    }

    fn check_subpicture_stream(
        chunks: &[SubChunk],
        expected: &[(&[u8], i64)],
    ) -> Result<(), MuxError> {
        let mut iter = chunks.iter();

        for (data, pts) in expected {
            let mut remaining = data.len();
            let mut first = true;
            let mut collected = Vec::new();

            while remaining > 0 {
                let chunk = iter
                    .next()
                    .ok_or_else(|| MuxError::Invalid("missing subpicture chunk".to_string()))?;
                if chunk.data.is_empty() {
                    return Err(MuxError::Invalid("empty subpicture chunk".to_string()));
                }

                if first {
                    let chunk_pts = chunk
                        .pts
                        .ok_or_else(|| MuxError::Invalid("missing subpicture PTS".to_string()))?;
                    if chunk_pts != *pts / 300 {
                        return Err(MuxError::Invalid("bad subpicture PTS".to_string()));
                    }
                } else if chunk.pts.is_some() {
                    return Err(MuxError::Invalid("unexpected subpicture PTS".to_string()));
                }

                if chunk.data.len() > remaining {
                    return Err(MuxError::Invalid(
                        "subpicture chunk crosses unit".to_string(),
                    ));
                }

                collected.extend_from_slice(&chunk.data);
                remaining -= chunk.data.len();
                first = false;
            }

            if collected.as_slice() != *data {
                return Err(MuxError::Invalid("subpicture payload mismatch".to_string()));
            }
        }

        if iter.next().is_some() {
            return Err(MuxError::Invalid("extra subpicture chunks".to_string()));
        }

        Ok(())
    }

    fn first_chunk_of_unit(
        chunks: &[SubChunk],
        unit_index: usize,
        expected: &[usize],
    ) -> Result<u32, MuxError> {
        let mut chunk_idx = 0usize;

        for (i, len) in expected.iter().enumerate() {
            let mut remaining = *len;
            let mut first = true;

            while remaining > 0 {
                let chunk = chunks
                    .get(chunk_idx)
                    .ok_or_else(|| MuxError::Invalid("missing subpicture chunk".to_string()))?;
                if chunk.data.is_empty() {
                    return Err(MuxError::Invalid("empty subpicture chunk".to_string()));
                }

                if first && i == unit_index {
                    return Ok(chunk.sector);
                }

                if chunk.data.len() > remaining {
                    return Err(MuxError::Invalid(
                        "subpicture chunk crosses unit".to_string(),
                    ));
                }

                remaining -= chunk.data.len();
                first = false;
                chunk_idx = chunk_idx
                    .checked_add(1)
                    .ok_or_else(|| MuxError::Invalid("chunk index overflow".to_string()))?;
            }
        }

        Err(MuxError::Invalid("missing subpicture unit".to_string()))
    }

    fn check_sp_synca(
        sink: &MemorySink,
        layout: &VobsLayout,
        stream: usize,
        first_sector: u32,
    ) -> Result<(), MuxError> {
        let vobu = layout
            .vobus
            .iter()
            .find(|v| v.start_sector <= first_sector && first_sector <= v.last_sector)
            .ok_or_else(|| MuxError::Invalid("subpicture pack outside VOBU".to_string()))?;
        let nav_idx = usize::try_from(vobu.start_sector)
            .map_err(|_| MuxError::Invalid("sector index overflow".to_string()))?;
        let nav = sink
            .sectors
            .get(nav_idx)
            .ok_or_else(|| MuxError::Invalid("missing NAV sector".to_string()))?;
        let offset = first_sector
            .checked_sub(vobu.start_sector)
            .ok_or_else(|| MuxError::Invalid("bad SP_SYNCA offset".to_string()))?;
        let value = read_u32(nav, DSI_DATA + 0x1A2 + 4 * stream)
            .ok_or_else(|| MuxError::Invalid("missing SP_SYNCA".to_string()))?;

        if value != offset {
            return Err(MuxError::Invalid("bad SP_SYNCA".to_string()));
        }

        Ok(())
    }

    fn basic_input<'a>(
        video: &'a VideoStream,
        video_data: &'a [u8],
        audio: Vec<AudioSource<'a>>,
        cell_starts: Vec<i64>,
    ) -> MuxInput<'a> {
        MuxInput {
            video,
            video_data,
            audio,
            audio_start_pts: FIRST_PTS,
            subpictures: Vec::new(),
            cell_starts,
            vob_id: 1,
            ntsc: true,
        }
    }

    fn mux_basic() -> Result<(Vec<u8>, Vec<u8>, VideoStream, VobsLayout, MemorySink), MuxError> {
        let video_data = build_test_video();
        let ac3_data = build_ac3_data(64);
        let video = parse_video_data(&video_data)?;
        let ac3 = parse_ac3_data(&ac3_data, FIRST_PTS)?;
        let input = basic_input(
            &video,
            &video_data,
            vec![AudioSource::Ac3 {
                stream: &ac3,
                data: &ac3_data,
            }],
            vec![0, GOP2_RELATIVE],
        );
        let mut sink = MemorySink::default();
        let layout = mux_vobs(&input, &mut sink)?;
        Ok((video_data, ac3_data, video, layout, sink))
    }

    #[test]
    fn sectors_are_packs_and_vobus_start_with_nav() -> Result<(), MuxError> {
        let (_video_data, _ac3_data, _video, layout, sink) = mux_basic()?;

        for sector in sink.sectors.iter() {
            if sector.len() != SECTOR {
                return Err(MuxError::Invalid("bad sector length".to_string()));
            }
            if sector.get(0..4) != Some(&[0, 0, 1, 0xBA]) {
                return Err(MuxError::Invalid("missing pack start code".to_string()));
            }
        }

        let mut prev_scr: Option<i64> = None;
        for sector in sink.sectors.iter() {
            let scr = scr_from_sector(sector)
                .ok_or_else(|| MuxError::Invalid("cannot decode SCR".to_string()))?;
            if let Some(prev) = prev_scr
                && scr <= prev
            {
                return Err(MuxError::Invalid("SCR does not increase".to_string()));
            }
            prev_scr = Some(scr);
        }

        for vobu in layout.vobus.iter() {
            let sector = sink
                .sectors
                .get(
                    usize::try_from(vobu.start_sector)
                        .map_err(|_| MuxError::Invalid("sector index overflow".to_string()))?,
                )
                .ok_or_else(|| MuxError::Invalid("missing VOBU sector".to_string()))?;

            if sector.get(14..18) != Some(&[0, 0, 1, 0xBB]) {
                return Err(MuxError::Invalid("missing system header".to_string()));
            }
            if sector.get(38..42) != Some(&[0, 0, 1, 0xBF]) {
                return Err(MuxError::Invalid("missing PCI packet".to_string()));
            }
            if sector.get(44) != Some(&0) {
                return Err(MuxError::Invalid("bad PCI sub stream".to_string()));
            }
            if sector.get(1024..1028) != Some(&[0, 0, 1, 0xBF]) {
                return Err(MuxError::Invalid("missing DSI packet".to_string()));
            }
            if sector.get(1030) != Some(&1) {
                return Err(MuxError::Invalid("bad DSI sub stream".to_string()));
            }
            let lbn = read_u32(sector, PCI_DATA)
                .ok_or_else(|| MuxError::Invalid("missing PCI LBN".to_string()))?;
            if lbn != vobu.start_sector {
                return Err(MuxError::Invalid("bad NV_PCK_LBN".to_string()));
            }
        }

        Ok(())
    }

    #[test]
    fn payloads_reproduce_elementary_streams() -> Result<(), MuxError> {
        let (video_data, ac3_data, video, _layout, sink) = mux_basic()?;

        let mut video_payload = Vec::new();
        let mut ac3_payload = Vec::new();

        for sector in sink.sectors.iter() {
            if sector.get(17) == Some(&0xBB) {
                continue;
            }
            let stream_id = sector
                .get(17)
                .copied()
                .ok_or_else(|| MuxError::Invalid("missing stream id".to_string()))?;
            let packet_len = read_u16(sector, 18)
                .ok_or_else(|| MuxError::Invalid("missing packet length".to_string()))?
                as usize;
            let hdr_len = sector
                .get(22)
                .copied()
                .ok_or_else(|| MuxError::Invalid("missing header length".to_string()))?
                as usize;
            if packet_len < 3 + hdr_len {
                return Err(MuxError::Invalid("bad PES packet length".to_string()));
            }
            let payload_len = packet_len - 3 - hdr_len;
            let payload_off: usize = 23 + hdr_len;
            let payload_end = payload_off
                .checked_add(payload_len)
                .ok_or_else(|| MuxError::Invalid("payload overflow".to_string()))?;
            let payload = sector
                .get(payload_off..payload_end)
                .ok_or_else(|| MuxError::Invalid("payload out of range".to_string()))?;

            if stream_id == 0xE0 {
                video_payload.extend_from_slice(payload);
            } else if stream_id == 0xBD {
                let sub = payload
                    .first()
                    .copied()
                    .ok_or_else(|| MuxError::Invalid("missing audio sub stream".to_string()))?;
                if (0x80..=0x87).contains(&sub) {
                    let audio = payload
                        .get(4..)
                        .ok_or_else(|| MuxError::Invalid("missing AC-3 payload".to_string()))?;
                    ac3_payload.extend_from_slice(audio);
                }
            }
        }

        let mut expected_video = video_data.clone();
        if !video.has_sequence_end {
            expected_video.extend_from_slice(&[0, 0, 1, 0xB7]);
        }

        if video_payload != expected_video {
            return Err(MuxError::Invalid("video payload mismatch".to_string()));
        }
        if ac3_payload != ac3_data {
            return Err(MuxError::Invalid("AC-3 payload mismatch".to_string()));
        }

        Ok(())
    }

    #[test]
    fn cells_split_at_requested_gop() -> Result<(), MuxError> {
        let (_video_data, _ac3_data, _video, layout, _sink) = mux_basic()?;

        if layout.cells.len() != 2 {
            return Err(MuxError::Invalid("expected two cells".to_string()));
        }
        let second = layout
            .cells
            .get(1)
            .ok_or_else(|| MuxError::Invalid("missing second cell".to_string()))?;
        if second.cell_id != 2 {
            return Err(MuxError::Invalid("bad second cell id".to_string()));
        }
        let third_vobu = layout
            .vobus
            .get(2)
            .ok_or_else(|| MuxError::Invalid("missing third VOBU".to_string()))?;
        if second.first_sector != third_vobu.start_sector {
            return Err(MuxError::Invalid(
                "cell 2 does not start at third VOBU".to_string(),
            ));
        }

        Ok(())
    }

    #[test]
    fn two_chapters_in_one_gop_is_error() -> Result<(), MuxError> {
        let video_data = build_test_video();
        let video = parse_video_data(&video_data)?;
        let input = basic_input(&video, &video_data, Vec::new(), vec![0, 1_000_000]);
        let mut sink = MemorySink::default();
        if mux_vobs(&input, &mut sink).is_ok() {
            return Err(MuxError::Invalid("expected same-GOP error".to_string()));
        }
        Ok(())
    }

    #[test]
    fn open_first_gop_is_error() -> Result<(), MuxError> {
        let video_data = build_test_video();
        let mut video = parse_video_data(&video_data)?;
        if let Some(pic) = video.pictures.get_mut(0) {
            pic.closed_gop = false;
        }
        let input = basic_input(&video, &video_data, Vec::new(), vec![0]);
        let mut sink = MemorySink::default();
        if mux_vobs(&input, &mut sink).is_ok() {
            return Err(MuxError::Invalid("expected open GOP error".to_string()));
        }
        Ok(())
    }

    #[test]
    fn vobu_ea_and_next_sri_are_consistent() -> Result<(), MuxError> {
        let (_video_data, _ac3_data, _video, layout, sink) = mux_basic()?;

        for (idx, vobu) in layout.vobus.iter().enumerate() {
            let sector = sink
                .sectors
                .get(
                    usize::try_from(vobu.start_sector)
                        .map_err(|_| MuxError::Invalid("sector index overflow".to_string()))?,
                )
                .ok_or_else(|| MuxError::Invalid("missing VOBU sector".to_string()))?;

            let ea = read_u32(sector, DSI_DATA + 0x08)
                .ok_or_else(|| MuxError::Invalid("missing VOBU_EA".to_string()))?;
            let expected_ea = vobu
                .last_sector
                .checked_sub(vobu.start_sector)
                .ok_or_else(|| MuxError::Invalid("bad VOBU range".to_string()))?;
            if ea != expected_ea {
                return Err(MuxError::Invalid("bad VOBU_EA".to_string()));
            }

            let next = read_u32(sector, DSI_DATA + 0xEA)
                .ok_or_else(|| MuxError::Invalid("missing next SRI".to_string()))?;
            let same_cell_next = layout
                .vobus
                .get(idx + 1)
                .map(|next_vobu| next_vobu.cell == vobu.cell)
                .unwrap_or(false);

            if same_cell_next {
                let next_vobu = layout
                    .vobus
                    .get(idx + 1)
                    .ok_or_else(|| MuxError::Invalid("missing next VOBU".to_string()))?;
                let offset = next_vobu
                    .start_sector
                    .checked_sub(vobu.start_sector)
                    .ok_or_else(|| MuxError::Invalid("bad next offset".to_string()))?;
                let expected = 0x8000_0000 | offset;
                if next != expected {
                    return Err(MuxError::Invalid("bad next SRI".to_string()));
                }
            } else if next != 0x3FFF_FFFF {
                return Err(MuxError::Invalid(
                    "last next SRI should be none".to_string(),
                ));
            }
        }

        Ok(())
    }

    #[test]
    fn lpcm_payloads_are_frame_aligned() -> Result<(), MuxError> {
        let video_data = build_test_video();
        let video = parse_video_data(&video_data)?;
        let lpcm = LpcmStream {
            sample_rate: 48_000,
            channels: 2,
            bits: 16,
            data: vec![0u8; 320 * 10],
            frame_bytes: 320,
        };
        let input = basic_input(&video, &video_data, vec![AudioSource::Lpcm(&lpcm)], vec![0]);
        let mut sink = MemorySink::default();
        let _layout = mux_vobs(&input, &mut sink)?;

        for sector in sink.sectors.iter() {
            if sector.get(17) == Some(&0xBB) {
                continue;
            }
            let stream_id = sector
                .get(17)
                .copied()
                .ok_or_else(|| MuxError::Invalid("missing stream id".to_string()))?;
            if stream_id != 0xBD {
                continue;
            }
            let packet_len = read_u16(sector, 18)
                .ok_or_else(|| MuxError::Invalid("missing packet length".to_string()))?
                as usize;
            let hdr_len = sector
                .get(22)
                .copied()
                .ok_or_else(|| MuxError::Invalid("missing header length".to_string()))?
                as usize;
            if packet_len < 3 + hdr_len + 7 {
                return Err(MuxError::Invalid("bad LPCM packet".to_string()));
            }
            let payload_len = packet_len - 3 - hdr_len;
            let payload_off = 23 + hdr_len;
            let payload_end = payload_off
                .checked_add(payload_len)
                .ok_or_else(|| MuxError::Invalid("payload overflow".to_string()))?;
            let payload = sector
                .get(payload_off..payload_end)
                .ok_or_else(|| MuxError::Invalid("payload out of range".to_string()))?;
            let sub = payload
                .first()
                .copied()
                .ok_or_else(|| MuxError::Invalid("missing LPCM sub stream".to_string()))?;
            if !(0xA0..=0xA7).contains(&sub) {
                continue;
            }
            let audio_len = payload_len - 7;
            if audio_len == 0 || !audio_len.is_multiple_of(4) {
                return Err(MuxError::Invalid("bad LPCM payload length".to_string()));
            }
        }

        Ok(())
    }

    #[test]
    fn subpicture_streams_are_muxed_and_recorded() -> Result<(), MuxError> {
        let video_data = build_test_video();
        let ac3_data = build_ac3_data(64);
        let video = parse_video_data(&video_data)?;
        let ac3 = parse_ac3_data(&ac3_data, FIRST_PTS)?;

        let mut gop_starts = video
            .pictures
            .iter()
            .filter(|pic| pic.gop_start)
            .map(|pic| pic.pts);
        let first_pts = gop_starts
            .next()
            .ok_or_else(|| MuxError::Invalid("missing first GOP".to_string()))?;
        let second_pts = gop_starts
            .next()
            .ok_or_else(|| MuxError::Invalid("missing second GOP".to_string()))?;

        let unit0 = vec![0x11u8; 3000];
        let unit1 = vec![0x22u8; 100];
        let unit2 = vec![0x33u8; 200];

        let subpictures = vec![
            vec![
                SubpictureUnit {
                    pts: first_pts,
                    data: unit0.clone(),
                },
                SubpictureUnit {
                    pts: second_pts,
                    data: unit1.clone(),
                },
            ],
            vec![SubpictureUnit {
                pts: first_pts,
                data: unit2.clone(),
            }],
        ];

        let input = MuxInput {
            video: &video,
            video_data: &video_data,
            audio: vec![AudioSource::Ac3 {
                stream: &ac3,
                data: &ac3_data,
            }],
            audio_start_pts: FIRST_PTS,
            subpictures,
            cell_starts: vec![0, GOP2_RELATIVE],
            vob_id: 1,
            ntsc: true,
        };

        let mut sink = MemorySink::default();
        let layout = mux_vobs(&input, &mut sink)?;

        let (video_payload, ac3_payload) = extract_video_ac3_payloads(&sink)?;
        let mut expected_video = video_data.clone();
        if !video.has_sequence_end {
            expected_video.extend_from_slice(&[0, 0, 1, 0xB7]);
        }
        if video_payload != expected_video {
            return Err(MuxError::Invalid("video payload mismatch".to_string()));
        }
        if ac3_payload != ac3_data {
            return Err(MuxError::Invalid("AC-3 payload mismatch".to_string()));
        }

        let chunks0 = extract_subpicture_chunks(&sink, 0x20)?;
        check_subpicture_stream(&chunks0, &[(&unit0, first_pts), (&unit1, second_pts)])?;

        let chunks1 = extract_subpicture_chunks(&sink, 0x21)?;
        check_subpicture_stream(&chunks1, &[(&unit2, first_pts)])?;

        let first0 = first_chunk_of_unit(&chunks0, 0, &[unit0.len(), unit1.len()])?;
        let first1 = first_chunk_of_unit(&chunks0, 1, &[unit0.len(), unit1.len()])?;
        let first2 = first_chunk_of_unit(&chunks1, 0, &[unit2.len()])?;

        check_sp_synca(&sink, &layout, 0, first0)?;
        check_sp_synca(&sink, &layout, 0, first1)?;
        check_sp_synca(&sink, &layout, 1, first2)?;

        Ok(())
    }

    #[test]
    fn vob_file_sink_splits_and_overwrites() -> Result<(), MuxError> {
        let dir = std::env::temp_dir().join(format!("disccraft-psmux-sink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;

        let mut sink = VobFileSink::new(&dir, 1);
        sink.max_sectors = 5;

        for i in 0..12u32 {
            let mut sector = [0u8; SECTOR];
            if let Some(byte) = sector.get_mut(0) {
                *byte = i as u8;
            }
            sink.write_sector(&sector)?;
        }
        sink.finish()?;

        let files = sink.files();
        if files.len() != 3 {
            return Err(MuxError::Invalid("expected three VOB files".to_string()));
        }

        let mut sector = [0u8; SECTOR];
        if let Some(byte) = sector.get_mut(0) {
            *byte = 99;
        }
        sink.overwrite_sector(7, &sector)?;

        let path = files
            .get(1)
            .ok_or_else(|| MuxError::Invalid("missing second VOB file".to_string()))?;
        let mut file = std::fs::File::open(path)?;
        file.seek(SeekFrom::Start(2 * SECTOR as u64))?;
        let mut buf = [0u8; SECTOR];
        file.read_exact(&mut buf)?;
        if buf.first().copied() != Some(99) {
            return Err(MuxError::Invalid(
                "overwrite landed in wrong place".to_string(),
            ));
        }

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[allow(dead_code)]
    fn unused_cell_layout(layout: &CellLayout) -> u8 {
        layout.cell_id
    }
}
