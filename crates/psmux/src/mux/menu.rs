//! Menu VOB muxer.

use super::hli::{Hli, write_hli};
use super::pack;
use super::{MuxError, invalid, u32_from_i64};
use crate::es::{Picture, PictureType, VideoStream, to_90k};
use crate::sink::{SECTOR, SectorSink};

/// Input for one menu VOB.
#[derive(Debug)]
pub struct MenuVobInput<'a> {
    /// Parsed MPEG-2 video stream containing exactly one I picture.
    pub video: &'a VideoStream,
    /// Video elementary stream bytes, including sequence end.
    pub video_data: &'a [u8],
    /// One subpicture unit for private stream 1 sub stream 0x20.
    pub spu: &'a [u8],
    /// Highlight information to write into the NAV pack.
    pub hli: &'a Hli,
    /// VOB id written into NAV packs.
    pub vob_id: u16,
    /// True for NTSC, false for PAL.
    pub ntsc: bool,
    /// Logical block number of this VOB within the VMGM VOBS.
    pub first_sector: u32,
}

/// Layout of one menu VOB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuVobLayout {
    /// First sector of the VOB.
    pub first_sector: u32,
    /// Last sector of the VOB.
    pub last_sector: u32,
    /// Number of sectors in the VOB.
    pub sectors: u32,
    /// VOBU start presentation time, 90 kHz.
    pub s_ptm: u32,
    /// VOBU end presentation time, 90 kHz.
    pub e_ptm: u32,
}

const SYSTEM_HEADER: &[u8] = &[
    0x00, 0x00, 0x01, 0xBB, 0x00, 0x12, 0x80, 0xC4, 0xE1, 0x04, 0xE1, 0xFF, 0xB9, 0xE0, 0xE8, 0xB8,
    0xE0, 0x08, 0xBD, 0xE0, 0x3A, 0xBF, 0xE0, 0x02,
];
const PCI_HEADER: &[u8] = &[0x00, 0x00, 0x01, 0xBF, 0x03, 0xD4, 0x00];
const DSI_HEADER: &[u8] = &[0x00, 0x00, 0x01, 0xBF, 0x03, 0xFA, 0x01];

const PCI_DATA: usize = 38 + PCI_HEADER.len();
const DSI_DATA: usize = 1024 + DSI_HEADER.len();
const PCI_DATA_LEN: usize = 979;

const PCI_LBN: usize = PCI_DATA;
const PCI_S_PTM: usize = PCI_DATA + 0x0C;
const PCI_E_PTM: usize = PCI_DATA + 0x10;
const PCI_SE_E_PTM: usize = PCI_DATA + 0x14;
const PCI_C_ELTM: usize = PCI_DATA + 0x18;

const DSI_SCR: usize = DSI_DATA;
const DSI_LBN: usize = DSI_DATA + 0x04;
const DSI_EA: usize = DSI_DATA + 0x08;
const DSI_1STREF: usize = DSI_DATA + 0x0C;
const DSI_2NDREF: usize = DSI_DATA + 0x10;
const DSI_3RDREF: usize = DSI_DATA + 0x14;
const DSI_VOB_ID: usize = DSI_DATA + 0x18;
const DSI_CELL_ID: usize = DSI_DATA + 0x1B;
const DSI_C_ELTM: usize = DSI_DATA + 0x1C;
const DSI_VOB_S_PTM: usize = DSI_DATA + 0x2C;
const DSI_VOB_E_PTM: usize = DSI_DATA + 0x30;
const DSI_SRI: usize = DSI_DATA + 0xEA;
const DSI_A_SYNCA: usize = DSI_DATA + 0x192;
const DSI_SP_SYNCA: usize = DSI_DATA + 0x1A2;

const PACK_NUM: i128 = 307_200;
const SCR_DEN: i128 = 7;
const LEAD_TIME: i128 = 1_350_000;

const MAX_VIDEO_PAYLOAD: usize = 1800;
const MAX_SPU_PAYLOAD: usize = 1800;

const VIDEO_STREAM_ID: u8 = 0xE0;
const PRIVATE_STREAM_ID: u8 = 0xBD;
const SPU_SUB_STREAM: u8 = 0x20;

const PES_FLAGS_NONE: u8 = 0x00;
const PES_FLAGS_PTS: u8 = 0x80;
const PES_FLAGS_PTS_DTS: u8 = 0xC0;

const SRI_NONE: u32 = 0x3FFF_FFFF;
const A_SYNCA_NONE: u16 = 0x3FFF;

const NTSC_FRAME_90K: u32 = 3003;
const PAL_FRAME_90K: u32 = 3600;

/// Mux one menu VOB into `sink`.
pub fn mux_menu_vob(
    input: &MenuVobInput<'_>,
    sink: &mut dyn SectorSink,
) -> Result<MenuVobLayout, MuxError> {
    let picture = validate_video(input)?;
    let pts = u32_from_i64(to_90k(picture.pts))?;
    let frame = if input.ntsc {
        NTSC_FRAME_90K
    } else {
        PAL_FRAME_90K
    };
    let e_ptm = pts
        .checked_add(frame)
        .ok_or_else(|| invalid("e_ptm overflow"))?;

    let video_packs = pack_count(input.video_data.len(), MAX_VIDEO_PAYLOAD)?;
    let spu_packs = pack_count(input.spu.len(), MAX_SPU_PAYLOAD)?;
    let sectors = 1u32
        .checked_add(video_packs)
        .and_then(|v| v.checked_add(spu_packs))
        .ok_or_else(|| invalid("sector count overflow"))?;

    check_timing(picture.dts, video_packs)?;

    write_placeholder(sink)?;
    write_video_packs(input, picture, video_packs, sink)?;

    let first_spu = 1u32
        .checked_add(video_packs)
        .ok_or_else(|| invalid("pack index overflow"))?;
    write_spu_packs(input, picture.pts, first_spu, spu_packs, sink)?;

    let nav_sector = build_nav(input, pts, e_ptm, sectors, video_packs, first_spu)?;
    sink.overwrite_sector(input.first_sector, &nav_sector)?;
    sink.finish()?;

    let last_sector = input
        .first_sector
        .checked_add(
            sectors
                .checked_sub(1)
                .ok_or_else(|| invalid("sector count underflow"))?,
        )
        .ok_or_else(|| invalid("last sector overflow"))?;

    Ok(MenuVobLayout {
        first_sector: input.first_sector,
        last_sector,
        sectors,
        s_ptm: pts,
        e_ptm,
    })
}

fn validate_video<'a>(input: &'a MenuVobInput<'_>) -> Result<&'a Picture, MuxError> {
    if input.video.pictures.len() != 1 {
        return Err(invalid("menu VOB must contain exactly one picture"));
    }
    let picture = input
        .video
        .pictures
        .first()
        .ok_or_else(|| invalid("missing picture"))?;
    if picture.kind != PictureType::I {
        return Err(invalid("menu picture must be an I picture"));
    }
    if input.video_data.is_empty() {
        return Err(invalid("empty video data"));
    }
    if input.spu.is_empty() {
        return Err(invalid("empty subpicture"));
    }

    let pic_end = picture
        .offset
        .checked_add(picture.len)
        .ok_or_else(|| invalid("picture length overflow"))?;
    if pic_end > input.video_data.len() {
        return Err(invalid("picture data out of range"));
    }

    Ok(picture)
}

fn pack_count(len: usize, chunk: usize) -> Result<u32, MuxError> {
    if len == 0 {
        return Err(invalid("empty payload"));
    }
    if chunk == 0 {
        return Err(invalid("zero chunk"));
    }

    let last = chunk
        .checked_sub(1)
        .ok_or_else(|| invalid("chunk underflow"))?;
    let packs = len
        .checked_add(last)
        .and_then(|v| v.checked_div(chunk))
        .ok_or_else(|| invalid("pack count overflow"))?;

    u32::try_from(packs).map_err(|_| invalid("too many packs"))
}

fn check_timing(dts: i64, video_packs: u32) -> Result<(), MuxError> {
    if dts < 0 {
        return Err(invalid("negative DTS"));
    }
    let last_scr = scr_for_pack(video_packs)?;
    let min_dts = last_scr
        .checked_add(LEAD_TIME)
        .ok_or_else(|| invalid("timing overflow"))?;
    if i128::from(dts) < min_dts {
        return Err(invalid("video DTS too early for mux rate"));
    }
    Ok(())
}

fn scr_for_pack(index: u32) -> Result<i128, MuxError> {
    let i = i128::from(index);
    let total = i
        .checked_mul(PACK_NUM)
        .ok_or_else(|| invalid("SCR overflow"))?;
    total
        .checked_div(SCR_DEN)
        .ok_or_else(|| invalid("SCR division"))
}

fn bcd_zero(ntsc: bool) -> u32 {
    let rate = if ntsc { 3 } else { 1 };
    rate << 6
}

fn write_placeholder(sink: &mut dyn SectorSink) -> Result<(), MuxError> {
    let sector = [0u8; SECTOR];
    sink.write_sector(&sector)?;
    Ok(())
}

fn write_video_packs(
    input: &MenuVobInput<'_>,
    picture: &Picture,
    video_packs: u32,
    sink: &mut dyn SectorSink,
) -> Result<(), MuxError> {
    let mut index = 1u32;
    let mut offset = 0usize;
    let total = input.video_data.len();

    while offset < total {
        let remaining = total
            .checked_sub(offset)
            .ok_or_else(|| invalid("offset overflow"))?;
        let chunk = if remaining > MAX_VIDEO_PAYLOAD {
            MAX_VIDEO_PAYLOAD
        } else {
            remaining
        };
        let end = offset
            .checked_add(chunk)
            .ok_or_else(|| invalid("offset overflow"))?;
        let payload = input
            .video_data
            .get(offset..end)
            .ok_or_else(|| invalid("video payload out of range"))?;

        let scr = scr_for_pack(index)?;
        let mut sector = [0u8; SECTOR];
        if index == 1 {
            write_video_first(&mut sector, scr, picture, payload)?;
        } else {
            pack::write_pes_pack(
                &mut sector,
                scr,
                VIDEO_STREAM_ID,
                PES_FLAGS_NONE,
                0,
                &[],
                payload,
            )?;
        }

        sink.write_sector(&sector)?;
        offset = end;
        index = index
            .checked_add(1)
            .ok_or_else(|| invalid("pack index overflow"))?;
    }

    let expected = video_packs
        .checked_add(1)
        .ok_or_else(|| invalid("pack count overflow"))?;
    if index != expected {
        return Err(invalid("video pack count mismatch"));
    }
    Ok(())
}

fn write_video_first(
    sector: &mut [u8; SECTOR],
    scr: i128,
    picture: &Picture,
    payload: &[u8],
) -> Result<(), MuxError> {
    let has_dts = picture.dts != picture.pts;
    let optional = pack::pts_bytes(to_90k(picture.pts), to_90k(picture.dts), has_dts)?;
    let flags2 = if has_dts {
        PES_FLAGS_PTS_DTS
    } else {
        PES_FLAGS_PTS
    };
    pack::write_pes_pack(
        sector,
        scr,
        VIDEO_STREAM_ID,
        flags2,
        optional.len(),
        &optional,
        payload,
    )
}

fn write_spu_packs(
    input: &MenuVobInput<'_>,
    pts: i64,
    first_index: u32,
    spu_packs: u32,
    sink: &mut dyn SectorSink,
) -> Result<(), MuxError> {
    let mut index = first_index;
    let mut offset = 0usize;
    let total = input.spu.len();

    while offset < total {
        let remaining = total
            .checked_sub(offset)
            .ok_or_else(|| invalid("offset overflow"))?;
        let chunk = if remaining > MAX_SPU_PAYLOAD {
            MAX_SPU_PAYLOAD
        } else {
            remaining
        };
        let end = offset
            .checked_add(chunk)
            .ok_or_else(|| invalid("offset overflow"))?;
        let spu_chunk = input
            .spu
            .get(offset..end)
            .ok_or_else(|| invalid("SPU payload out of range"))?;

        let mut payload = Vec::with_capacity(
            chunk
                .checked_add(1)
                .ok_or_else(|| invalid("payload overflow"))?,
        );
        payload.push(SPU_SUB_STREAM);
        payload.extend_from_slice(spu_chunk);

        let scr = scr_for_pack(index)?;
        let mut sector = [0u8; SECTOR];
        if index == first_index {
            let optional = pack::pts_bytes(to_90k(pts), to_90k(pts), false)?;
            pack::write_pes_pack(
                &mut sector,
                scr,
                PRIVATE_STREAM_ID,
                PES_FLAGS_PTS,
                optional.len(),
                &optional,
                &payload,
            )?;
        } else {
            pack::write_pes_pack(
                &mut sector,
                scr,
                PRIVATE_STREAM_ID,
                PES_FLAGS_NONE,
                0,
                &[],
                &payload,
            )?;
        }

        sink.write_sector(&sector)?;
        offset = end;
        index = index
            .checked_add(1)
            .ok_or_else(|| invalid("pack index overflow"))?;
    }

    let expected = first_index
        .checked_add(spu_packs)
        .ok_or_else(|| invalid("pack count overflow"))?;
    if index != expected {
        return Err(invalid("SPU pack count mismatch"));
    }
    Ok(())
}

fn build_nav(
    input: &MenuVobInput<'_>,
    s_ptm: u32,
    e_ptm: u32,
    sectors: u32,
    video_packs: u32,
    first_spu: u32,
) -> Result<[u8; SECTOR], MuxError> {
    let mut sector = [0u8; SECTOR];
    pack::write_pack_header(&mut sector, 0)?;
    pack::put_slice(&mut sector, 14, SYSTEM_HEADER)?;
    pack::put_slice(&mut sector, 38, PCI_HEADER)?;
    pack::put_slice(&mut sector, 1024, DSI_HEADER)?;

    write_pci_gi(&mut sector, input, s_ptm, e_ptm)?;
    write_dsi(
        &mut sector,
        input,
        s_ptm,
        e_ptm,
        sectors,
        video_packs,
        first_spu,
    )?;

    Ok(sector)
}

fn write_pci_gi(
    sector: &mut [u8; SECTOR],
    input: &MenuVobInput<'_>,
    s_ptm: u32,
    e_ptm: u32,
) -> Result<(), MuxError> {
    pack::put_u32_be(sector, PCI_LBN, input.first_sector)?;
    pack::put_u32_be(sector, PCI_S_PTM, s_ptm)?;
    pack::put_u32_be(sector, PCI_E_PTM, e_ptm)?;

    let se_e_ptm = if input.video.has_sequence_end {
        e_ptm
    } else {
        0
    };
    pack::put_u32_be(sector, PCI_SE_E_PTM, se_e_ptm)?;

    let celtm = bcd_zero(input.ntsc);
    pack::put_u32_be(sector, PCI_C_ELTM, celtm)?;

    let pci_end = PCI_DATA
        .checked_add(PCI_DATA_LEN)
        .ok_or_else(|| invalid("PCI data end overflow"))?;
    let pci = sector
        .get_mut(PCI_DATA..pci_end)
        .ok_or_else(|| invalid("PCI data out of range"))?;

    let hli = Hli {
        start_ptm: s_ptm,
        buttons: input.hli.buttons.clone(),
        default_button: input.hli.default_button,
        button_offset: input.hli.button_offset,
        color_sets: input.hli.color_sets,
    };
    write_hli(&hli, pci)
}

fn write_dsi(
    sector: &mut [u8; SECTOR],
    input: &MenuVobInput<'_>,
    s_ptm: u32,
    e_ptm: u32,
    sectors: u32,
    video_packs: u32,
    first_spu: u32,
) -> Result<(), MuxError> {
    pack::put_u32_be(sector, DSI_SCR, 0)?;
    pack::put_u32_be(sector, DSI_LBN, input.first_sector)?;

    let vobu_ea = sectors
        .checked_sub(1)
        .ok_or_else(|| invalid("sector count underflow"))?;
    pack::put_u32_be(sector, DSI_EA, vobu_ea)?;

    pack::put_u32_be(sector, DSI_1STREF, video_packs)?;
    pack::put_u32_be(sector, DSI_2NDREF, 0)?;
    pack::put_u32_be(sector, DSI_3RDREF, 0)?;

    pack::put_u16_be(sector, DSI_VOB_ID, input.vob_id)?;
    pack::put_u8(sector, DSI_CELL_ID, 1)?;

    let celtm = bcd_zero(input.ntsc);
    pack::put_u32_be(sector, DSI_C_ELTM, celtm)?;
    pack::put_u32_be(sector, DSI_VOB_S_PTM, s_ptm)?;
    pack::put_u32_be(sector, DSI_VOB_E_PTM, e_ptm)?;

    write_sri(sector)?;
    write_a_synca(sector)?;
    write_sp_synca(sector, first_spu)
}

fn write_sri(sector: &mut [u8; SECTOR]) -> Result<(), MuxError> {
    for index in 0usize..42 {
        let delta = index
            .checked_mul(4)
            .ok_or_else(|| invalid("SRI index overflow"))?;
        let offset = DSI_SRI
            .checked_add(delta)
            .ok_or_else(|| invalid("SRI offset overflow"))?;
        pack::put_u32_be(sector, offset, SRI_NONE)?;
    }
    Ok(())
}

fn write_a_synca(sector: &mut [u8; SECTOR]) -> Result<(), MuxError> {
    for index in 0usize..8 {
        let delta = index
            .checked_mul(2)
            .ok_or_else(|| invalid("A_SYNCA index overflow"))?;
        let offset = DSI_A_SYNCA
            .checked_add(delta)
            .ok_or_else(|| invalid("A_SYNCA offset overflow"))?;
        pack::put_u16_be(sector, offset, A_SYNCA_NONE)?;
    }
    Ok(())
}

fn write_sp_synca(sector: &mut [u8; SECTOR], first_spu: u32) -> Result<(), MuxError> {
    pack::put_u32_be(sector, DSI_SP_SYNCA, first_spu)
}

#[cfg(test)]
mod tests {
    use super::{MenuVobInput, MenuVobLayout, MuxError, PCI_DATA, PCI_DATA_LEN, mux_menu_vob};
    use crate::es::{parse_video, to_90k};
    use crate::mux::hli::{Hli, HliButton, read_hli};
    use crate::sink::{MemorySink, SECTOR};

    const START_DTS: i64 = 8_100_000;

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
        bytes.extend_from_slice(&[0xAA; 100]);
    }

    fn write_one_picture(bytes: &mut Vec<u8>, tr: u32, coding: u32) {
        write_picture_header(bytes, tr, coding);
        write_picture_coding_extension(bytes);
        write_slice(bytes);
    }

    fn write_sequence_end(bytes: &mut Vec<u8>) {
        append_start_code(bytes, 0xB7);
    }

    fn build_menu_video() -> Vec<u8> {
        let mut data = Vec::new();
        write_sequence_header(&mut data);
        write_sequence_extension(&mut data);
        write_gop(&mut data, 1);
        write_one_picture(&mut data, 0, 1);
        write_sequence_end(&mut data);
        data
    }

    fn build_two_picture_video() -> Vec<u8> {
        let mut data = Vec::new();
        write_sequence_header(&mut data);
        write_sequence_extension(&mut data);
        write_gop(&mut data, 1);
        write_one_picture(&mut data, 0, 1);
        write_one_picture(&mut data, 3, 1);
        write_sequence_end(&mut data);
        data
    }

    fn test_button(up: u8, down: u8, left: u8, right: u8) -> HliButton {
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

    fn test_hli(start_ptm: u32) -> Hli {
        Hli {
            start_ptm,
            buttons: vec![
                test_button(2, 3, 1, 1),
                test_button(3, 1, 2, 2),
                test_button(1, 2, 3, 3),
            ],
            default_button: 1,
            button_offset: 0,
            color_sets: [[0u8; 8]; 3],
        }
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

    fn parse_menu_video(data: &[u8]) -> Result<crate::es::VideoStream, MuxError> {
        parse_video(data, START_DTS).map_err(|err| MuxError::Invalid(err.to_string()))
    }

    #[test]
    fn menu_vob_basic() -> Result<(), MuxError> {
        let video_data = build_menu_video();
        let video = parse_menu_video(&video_data)?;
        let picture = video
            .pictures
            .first()
            .ok_or_else(|| MuxError::Invalid("missing picture".to_string()))?;
        let s_ptm = u32::try_from(to_90k(picture.pts))
            .map_err(|_| MuxError::Invalid("PTS out of range".to_string()))?;

        let spu: Vec<u8> = (0u8..100).collect();
        let hli = test_hli(s_ptm);
        let input = MenuVobInput {
            video: &video,
            video_data: &video_data,
            spu: &spu,
            hli: &hli,
            vob_id: 1,
            ntsc: true,
            first_sector: 0,
        };

        let mut sink = MemorySink::default();
        let layout: MenuVobLayout = mux_menu_vob(&input, &mut sink)?;

        let sector_count = u32::try_from(sink.sectors.len())
            .map_err(|_| MuxError::Invalid("too many sectors".to_string()))?;
        if layout.sectors != sector_count {
            return Err(MuxError::Invalid(
                "layout sector count mismatch".to_string(),
            ));
        }
        if layout.first_sector != input.first_sector {
            return Err(MuxError::Invalid(
                "layout first sector mismatch".to_string(),
            ));
        }
        if layout.s_ptm != s_ptm {
            return Err(MuxError::Invalid("layout s_ptm mismatch".to_string()));
        }

        let sector0 = sink
            .sectors
            .first()
            .ok_or_else(|| MuxError::Invalid("missing NAV sector".to_string()))?;
        if sector0.get(0..4) != Some(&[0, 0, 1, 0xBA]) {
            return Err(MuxError::Invalid("missing NAV pack header".to_string()));
        }
        if sector0.get(14..18) != Some(&[0, 0, 1, 0xBB]) {
            return Err(MuxError::Invalid("missing NAV system header".to_string()));
        }

        let lbn = read_u32(sector0, PCI_DATA)
            .ok_or_else(|| MuxError::Invalid("missing PCI LBN".to_string()))?;
        if lbn != input.first_sector {
            return Err(MuxError::Invalid("bad NAV LBN".to_string()));
        }

        let pci = sector0
            .get(PCI_DATA..PCI_DATA + PCI_DATA_LEN)
            .ok_or_else(|| MuxError::Invalid("missing PCI data".to_string()))?;
        let got = read_hli(pci)?.ok_or_else(|| MuxError::Invalid("missing HLI".to_string()))?;
        if got != *input.hli {
            return Err(MuxError::Invalid("HLI mismatch".to_string()));
        }

        let mut spu_payload = Vec::new();
        let mut found = false;
        for sector in sink.sectors.iter().skip(1) {
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

            let packet_len = usize::from(
                read_u16(sector, 18)
                    .ok_or_else(|| MuxError::Invalid("missing PES length".to_string()))?,
            );
            let hdr_len = usize::from(
                sector
                    .get(22)
                    .copied()
                    .ok_or_else(|| MuxError::Invalid("missing PES header length".to_string()))?,
            );
            let payload_len = packet_len
                .checked_sub(3)
                .and_then(|v| v.checked_sub(hdr_len))
                .ok_or_else(|| MuxError::Invalid("bad PES packet length".to_string()))?;
            if payload_len < 1 {
                return Err(MuxError::Invalid("empty SPU payload".to_string()));
            }

            let payload_off = 23usize
                .checked_add(hdr_len)
                .ok_or_else(|| MuxError::Invalid("payload offset overflow".to_string()))?;
            let payload_end = payload_off
                .checked_add(payload_len)
                .ok_or_else(|| MuxError::Invalid("payload end overflow".to_string()))?;
            let payload = sector
                .get(payload_off..payload_end)
                .ok_or_else(|| MuxError::Invalid("payload out of range".to_string()))?;
            let sub = payload
                .first()
                .copied()
                .ok_or_else(|| MuxError::Invalid("missing SPU sub stream".to_string()))?;
            if sub == 0x20 {
                found = true;
                let chunk = payload
                    .get(1..)
                    .ok_or_else(|| MuxError::Invalid("missing SPU bytes".to_string()))?;
                spu_payload.extend_from_slice(chunk);
            }
        }

        if !found {
            return Err(MuxError::Invalid("missing SPU pack".to_string()));
        }
        if spu_payload != spu {
            return Err(MuxError::Invalid("SPU payload mismatch".to_string()));
        }

        Ok(())
    }

    #[test]
    fn two_pictures_is_error() -> Result<(), MuxError> {
        let video_data = build_two_picture_video();
        let video = parse_menu_video(&video_data)?;
        let spu: Vec<u8> = (0u8..100).collect();
        let hli = test_hli(0);
        let input = MenuVobInput {
            video: &video,
            video_data: &video_data,
            spu: &spu,
            hli: &hli,
            vob_id: 1,
            ntsc: true,
            first_sector: 0,
        };

        let mut sink = MemorySink::default();
        if mux_menu_vob(&input, &mut sink).is_ok() {
            return Err(MuxError::Invalid("expected two-picture error".to_string()));
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn unused_sector(sector: &[u8; SECTOR]) -> usize {
        sector.len()
    }
}
