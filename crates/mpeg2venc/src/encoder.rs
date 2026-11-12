#![allow(dead_code)]

use crate::bitwriter::BitWriter;
use crate::dct;
use crate::motion;
use crate::quant;
use crate::vlc;
use crate::{EncodeError, Frame420, Standard};
use std::sync::OnceLock;

const MAX_BITRATE_LIMIT: u32 = 9_800_000;
const VBV_BUFFER_BITS: u64 = 1_835_008;
const P_F_CODE: u8 = 2;

const INV_SQRT2: f64 = 1.0 / std::f64::consts::SQRT_2;

static COS_TABLE: OnceLock<[[f64; 8]; 8]> = OnceLock::new();

/// Encoder configuration for an I/P MPEG-2 sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderConfig {
    /// Target broadcast standard.
    pub standard: Standard,
    /// Frames per closed GOP.
    pub gop_size: u16,
    /// Average bitrate in bits per second.
    pub bitrate: u32,
    /// Maximum bitrate in bits per second, at most 9,800,000.
    pub max_bitrate: u32,
    /// 16:9 display (aspect_ratio_information 3) instead of 4:3 (2).
    pub wide: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PictureType {
    I,
    P,
}

struct Predictors {
    y: u16,
    cb: u16,
    cr: u16,
}

impl Predictors {
    fn new() -> Self {
        Self {
            y: 128,
            cb: 128,
            cr: 128,
        }
    }
}

/// MPEG-2 I/P sequence encoder.
pub struct Encoder {
    config: EncoderConfig,
    frames_encoded: u64,
    reference: Option<Frame420>,
    reconstruction: Option<Frame420>,
    quant_i: u8,
    quant_p: u8,
    vbv_fullness: u64,
}

impl Encoder {
    /// Create an encoder.
    pub fn new(config: EncoderConfig) -> Result<Self, EncodeError> {
        if config.gop_size == 0 {
            return Err(EncodeError("gop_size must be non-zero".to_string()));
        }

        if config.bitrate == 0 {
            return Err(EncodeError("bitrate must be non-zero".to_string()));
        }

        if config.max_bitrate == 0 {
            return Err(EncodeError("max_bitrate must be non-zero".to_string()));
        }

        if config.max_bitrate > MAX_BITRATE_LIMIT {
            return Err(EncodeError(format!(
                "max_bitrate must be <= {MAX_BITRATE_LIMIT}"
            )));
        }

        Ok(Self {
            config,
            frames_encoded: 0,
            reference: None,
            reconstruction: None,
            quant_i: 6,
            quant_p: 8,
            vbv_fullness: VBV_BUFFER_BITS,
        })
    }

    /// Encode one frame as one access unit.
    pub fn encode_frame(&mut self, frame: &Frame420) -> Result<Vec<u8>, EncodeError> {
        validate_frame(frame, self.config.standard)?;

        let gop_size = u64::from(self.config.gop_size);
        let is_gop_start = self.frames_encoded.is_multiple_of(gop_size);
        let picture_type = if is_gop_start {
            PictureType::I
        } else {
            PictureType::P
        };
        let temporal_reference = (self.frames_encoded % gop_size) as u32;
        let target_bits = self.target_bits(picture_type);

        let mut code = match picture_type {
            PictureType::I => self.quant_i,
            PictureType::P => self.quant_p,
        };

        let fill = self.fill_bits();
        let fullness = self.vbv_fullness.saturating_add(fill).min(VBV_BUFFER_BITS);
        let mut attempts = 0u32;
        let mut bits;

        let (data, recon) = loop {
            let encoded =
                self.encode_picture(frame, picture_type, is_gop_start, temporal_reference, code)?;

            bits = u64::try_from(encoded.0.len())
                .unwrap_or(u64::MAX)
                .saturating_mul(8);

            if bits <= fullness {
                break encoded;
            }

            if attempts >= 3 {
                return Err(EncodeError("VBV underflow".to_string()));
            }

            attempts += 1;
            code = code.saturating_add(4).min(31);
        };

        self.vbv_fullness = fullness.saturating_sub(bits);

        let updated_code = adjust_code(code, bits, target_bits);
        match picture_type {
            PictureType::I => self.quant_i = updated_code,
            PictureType::P => self.quant_p = updated_code,
        }

        self.frames_encoded += 1;
        self.reference = Some(recon.clone());
        self.reconstruction = Some(recon);

        Ok(data)
    }

    /// Return the sequence end code.
    pub fn finish(&mut self) -> Vec<u8> {
        let mut writer = BitWriter::new();
        writer.write_start_code(0xB7);
        writer.finish()
    }

    /// Last reconstructed frame, if any.
    pub fn reconstruction(&self) -> Option<&Frame420> {
        self.reconstruction.as_ref()
    }

    /// Number of frames encoded so far.
    pub fn frames_encoded(&self) -> u64 {
        self.frames_encoded
    }

    fn fps(&self) -> u32 {
        match self.config.standard {
            Standard::Ntsc => 30,
            Standard::Pal => 25,
        }
    }

    fn target_bits(&self, picture_type: PictureType) -> u64 {
        let fps = u64::from(self.fps());
        let average = (u64::from(self.config.bitrate) / fps).max(1);
        let gop = u64::from(self.config.gop_size);
        let p_target = ((average * gop) / (gop + 2).max(1)).max(1);

        match picture_type {
            PictureType::I => p_target.saturating_mul(3),
            PictureType::P => p_target,
        }
    }

    fn fill_bits(&self) -> u64 {
        let fps = u64::from(self.fps());
        (u64::from(self.config.max_bitrate) / fps).max(1)
    }

    fn encode_picture(
        &self,
        frame: &Frame420,
        picture_type: PictureType,
        is_gop_start: bool,
        temporal_reference: u32,
        code: u8,
    ) -> Result<(Vec<u8>, Frame420), EncodeError> {
        let mut writer = BitWriter::new();
        let mut recon = empty_frame(frame.width, frame.height);

        if is_gop_start {
            write_sequence_header(
                &mut writer,
                frame,
                self.config.standard,
                bit_rate_value(self.config.max_bitrate),
                if self.config.wide { 3 } else { 2 },
            );
            write_sequence_extension(&mut writer);
            write_gop_header(&mut writer, self.frames_encoded, self.fps());
        }

        write_picture_header(&mut writer, picture_type, temporal_reference);
        write_picture_coding_extension(&mut writer, picture_type);

        let qs = u16::from(code).saturating_mul(2);

        match picture_type {
            PictureType::I => {
                write_slices_intra(&mut writer, frame, &mut recon, code, qs);
            }
            PictureType::P => {
                let reference = self
                    .reference
                    .as_ref()
                    .ok_or_else(|| EncodeError("missing reference frame".to_string()))?;
                write_slices_p(&mut writer, frame, reference, &mut recon, code, qs);
            }
        }

        Ok((writer.finish(), recon))
    }
}

fn bit_rate_value(max_bitrate: u32) -> u32 {
    max_bitrate.saturating_add(399) / 400
}

fn adjust_code(code: u8, actual_bits: u64, target_bits: u64) -> u8 {
    if target_bits == 0 {
        return code;
    }

    let scaled = u64::from(code).saturating_mul(actual_bits);
    let rounded = (scaled + target_bits / 2) / target_bits;
    let mut updated = u8::try_from(rounded).unwrap_or(31);

    updated = updated.clamp(2, 31);

    let min_allowed = code.saturating_sub(4);
    let max_allowed = code.saturating_add(4);

    if updated < min_allowed {
        updated = min_allowed;
    }
    if updated > max_allowed {
        updated = max_allowed;
    }

    updated = updated.clamp(2, 31);

    updated
}

fn validate_frame(frame: &Frame420, standard: Standard) -> Result<(), EncodeError> {
    let w = usize::from(frame.width);
    let h = usize::from(frame.height);
    let expected_h = match standard {
        Standard::Ntsc => 480,
        Standard::Pal => 576,
    };

    if w != 720 || h != expected_h {
        return Err(EncodeError(format!(
            "expected 720x{expected_h}, got {w}x{h}"
        )));
    }

    let y_len = w
        .checked_mul(h)
        .ok_or_else(|| EncodeError("invalid frame size".to_string()))?;
    let cw = w / 2;
    let ch = h / 2;
    let c_len = cw
        .checked_mul(ch)
        .ok_or_else(|| EncodeError("invalid chroma size".to_string()))?;

    if frame.y.len() != y_len || frame.cb.len() != c_len || frame.cr.len() != c_len {
        return Err(EncodeError("plane length mismatch".to_string()));
    }

    Ok(())
}

fn empty_frame(width: u16, height: u16) -> Frame420 {
    let w = usize::from(width);
    let h = usize::from(height);
    let cw = w / 2;
    let ch = h / 2;

    Frame420 {
        width,
        height,
        y: vec![0u8; w * h],
        cb: vec![0u8; cw * ch],
        cr: vec![0u8; cw * ch],
    }
}

fn write_sequence_header(
    writer: &mut BitWriter,
    frame: &Frame420,
    standard: Standard,
    bit_rate_value: u32,
    aspect_code: u32,
) {
    let frame_rate_code = match standard {
        Standard::Ntsc => 4,
        Standard::Pal => 3,
    };

    writer.write_start_code(0xB3);
    writer.write_bits(u32::from(frame.width), 12);
    writer.write_bits(u32::from(frame.height), 12);
    writer.write_bits(aspect_code, 4);
    writer.write_bits(frame_rate_code, 4);
    writer.write_bits(bit_rate_value, 18);
    writer.write_bits(1, 1);
    writer.write_bits(112, 10);
    writer.write_bits(0, 1);
    writer.write_bits(0, 1);
    writer.write_bits(0, 1);
}

fn write_sequence_extension(writer: &mut BitWriter) {
    writer.write_start_code(0xB5);
    writer.write_bits(1, 4);
    writer.write_bits(0x48, 8);
    writer.write_bits(0, 1);
    writer.write_bits(1, 2);
    writer.write_bits(0, 12);
    writer.write_bits(0, 12);
    writer.write_bits(0, 4);
    writer.write_bits(0, 8);
    writer.write_bits(0, 1);
    writer.write_bits(0, 2);
    writer.write_bits(0, 5);
}

fn write_gop_header(writer: &mut BitWriter, total_frames: u64, fps: u32) {
    let fps = u64::from(fps);
    let hours = (total_frames / (3600 * fps)) as u32 & 0x1F;
    let minutes = ((total_frames / (60 * fps)) % 60) as u32;
    let seconds = ((total_frames / fps) % 60) as u32;
    let pictures = (total_frames % fps) as u32;

    writer.write_start_code(0xB8);
    // time_code (25 bits): drop_frame_flag, hours, minutes, marker_bit, seconds, pictures.
    writer.write_bits(0, 1);
    writer.write_bits(hours, 5);
    writer.write_bits(minutes, 6);
    writer.write_bits(1, 1);
    writer.write_bits(seconds, 6);
    writer.write_bits(pictures, 6);
    writer.write_bits(1, 1); // closed_gop
    writer.write_bits(0, 1); // broken_link
}

fn write_picture_header(
    writer: &mut BitWriter,
    picture_type: PictureType,
    temporal_reference: u32,
) {
    writer.write_start_code(0x00);
    writer.write_bits(temporal_reference, 10);

    match picture_type {
        PictureType::I => {
            writer.write_bits(1, 3);
            writer.write_bits(0xFFFF, 16);
        }
        PictureType::P => {
            writer.write_bits(2, 3);
            writer.write_bits(0xFFFF, 16);
            writer.write_bits(0, 1);
            writer.write_bits(7, 3);
        }
    }
}

fn write_picture_coding_extension(writer: &mut BitWriter, picture_type: PictureType) {
    let (f00, f01) = match picture_type {
        PictureType::I => (15, 15),
        PictureType::P => (2, 2),
    };

    writer.write_start_code(0xB5);
    writer.write_bits(8, 4);
    writer.write_bits(f00, 4);
    writer.write_bits(f01, 4);
    writer.write_bits(15, 4);
    writer.write_bits(15, 4);
    writer.write_bits(0, 2);
    writer.write_bits(3, 2);
    writer.write_bits(1, 1);
    writer.write_bits(1, 1);
    writer.write_bits(0, 1);
    writer.write_bits(0, 1);
    writer.write_bits(0, 1);
    writer.write_bits(0, 1);
    writer.write_bits(0, 1);
    writer.write_bits(1, 1);
    writer.write_bits(1, 1);
    writer.write_bits(0, 1);
}

fn write_slice_header(writer: &mut BitWriter, row: usize, code: u8) {
    let slice_code = u8::try_from(row + 1).unwrap_or(1);
    writer.write_start_code(slice_code);
    writer.write_bits(u32::from(code), 5);
    writer.write_bits(0, 1);
}

fn write_slices_intra(
    writer: &mut BitWriter,
    frame: &Frame420,
    recon: &mut Frame420,
    code: u8,
    qs: u16,
) {
    let w = usize::from(frame.width);
    let h = usize::from(frame.height);
    let mb_rows = h / 16;
    let mb_cols = w / 16;

    for row in 0..mb_rows {
        write_slice_header(writer, row, code);
        let mut predictors = Predictors::new();

        for col in 0..mb_cols {
            let x = col * 16;
            let y = row * 16;
            encode_intra_macroblock(writer, frame, recon, x, y, qs, &mut predictors);
        }
    }
}

fn write_slices_p(
    writer: &mut BitWriter,
    cur: &Frame420,
    reference: &Frame420,
    recon: &mut Frame420,
    code: u8,
    qs: u16,
) {
    let w = usize::from(cur.width);
    let h = usize::from(cur.height);
    let mb_rows = h / 16;
    let mb_cols = w / 16;

    for row in 0..mb_rows {
        write_slice_header(writer, row, code);
        let mut predictors = Predictors::new();
        let mut prev_intra = false;
        let mut pmv_x = 0i32;
        let mut pmv_y = 0i32;

        for col in 0..mb_cols {
            let x = col * 16;
            let y = row * 16;
            encode_p_macroblock(
                writer,
                cur,
                reference,
                recon,
                x,
                y,
                qs,
                &mut predictors,
                &mut prev_intra,
                &mut pmv_x,
                &mut pmv_y,
            );
        }
    }
}

fn encode_intra_macroblock(
    writer: &mut BitWriter,
    frame: &Frame420,
    recon: &mut Frame420,
    x: usize,
    y: usize,
    qs: u16,
    predictors: &mut Predictors,
) {
    writer.write_bits(1, 1);
    writer.write_bits(1, 1);
    encode_intra_blocks(writer, frame, recon, x, y, qs, predictors);
}

fn encode_intra_blocks(
    writer: &mut BitWriter,
    frame: &Frame420,
    recon: &mut Frame420,
    x: usize,
    y: usize,
    qs: u16,
    predictors: &mut Predictors,
) {
    let w = usize::from(frame.width);
    let cw = w / 2;

    encode_intra_block(
        writer,
        &frame.y,
        w,
        x,
        y,
        qs,
        true,
        &mut predictors.y,
        &mut recon.y,
        w,
        x,
        y,
    );
    encode_intra_block(
        writer,
        &frame.y,
        w,
        x + 8,
        y,
        qs,
        true,
        &mut predictors.y,
        &mut recon.y,
        w,
        x + 8,
        y,
    );
    encode_intra_block(
        writer,
        &frame.y,
        w,
        x,
        y + 8,
        qs,
        true,
        &mut predictors.y,
        &mut recon.y,
        w,
        x,
        y + 8,
    );
    encode_intra_block(
        writer,
        &frame.y,
        w,
        x + 8,
        y + 8,
        qs,
        true,
        &mut predictors.y,
        &mut recon.y,
        w,
        x + 8,
        y + 8,
    );

    encode_intra_block(
        writer,
        &frame.cb,
        cw,
        x / 2,
        y / 2,
        qs,
        false,
        &mut predictors.cb,
        &mut recon.cb,
        cw,
        x / 2,
        y / 2,
    );
    encode_intra_block(
        writer,
        &frame.cr,
        cw,
        x / 2,
        y / 2,
        qs,
        false,
        &mut predictors.cr,
        &mut recon.cr,
        cw,
        x / 2,
        y / 2,
    );
}

#[allow(clippy::too_many_arguments)] // TODO: group into a context struct (tech debt)
fn encode_intra_block(
    writer: &mut BitWriter,
    plane: &[u8],
    width: usize,
    x: usize,
    y: usize,
    qs: u16,
    is_luma: bool,
    predictor: &mut u16,
    recon_plane: &mut [u8],
    recon_width: usize,
    recon_x: usize,
    recon_y: usize,
) {
    let samples = block8x8(plane, width, x, y);
    let coeffs = dct::forward_dct8x8(&samples);
    let levels = quant::quantise_intra(&coeffs, qs);

    let dc = levels.first().copied().unwrap_or(0);
    let diff = i32::from(dc) - i32::from(*predictor);
    *predictor = u16::try_from(dc).unwrap_or(0);

    write_dc_diff(writer, diff, is_luma);
    write_intra_ac(writer, &levels);

    let recon_block = quant::reconstruct_intra(&levels, qs);
    copy_block8x8(recon_plane, recon_width, recon_x, recon_y, &recon_block);
}

/// Map the internal block mask (bit i = block i) to the bitstream order (block 0 = bit 5).
fn reverse_cbp(cbp: u8) -> u8 {
    (0..6).fold(0u8, |acc, i| acc | (((cbp >> i) & 1) << (5 - i)))
}

#[allow(clippy::too_many_arguments)] // TODO: group into a context struct (tech debt)
fn encode_p_macroblock(
    writer: &mut BitWriter,
    cur: &Frame420,
    reference: &Frame420,
    recon: &mut Frame420,
    x: usize,
    y: usize,
    qs: u16,
    predictors: &mut Predictors,
    prev_intra: &mut bool,
    pmv_x: &mut i32,
    pmv_y: &mut i32,
) {
    let (dx, dy, sad) = motion::search_fast(cur, reference, x, y, 15, (*pmv_x / 2, *pmv_y / 2));
    let intra = motion::intra_cost(cur, x, y);

    if intra.saturating_add(512) < sad {
        vlc::write_macroblock_address_increment(writer, 1);
        vlc::write_p_macroblock_type(writer, vlc::PType::Intra);

        if !*prev_intra {
            *predictors = Predictors::new();
        }

        encode_intra_blocks(writer, cur, recon, x, y, qs, predictors);

        *prev_intra = true;
        *pmv_x = 0;
        *pmv_y = 0;
        return;
    }

    let mv_x = dx.saturating_mul(2);
    let mv_y = dy.saturating_mul(2);
    let (pred_y, pred_cb, pred_cr) = motion::predict(reference, x, y, mv_x, mv_y);

    let w = usize::from(cur.width);
    let cw = w / 2;

    let levels0 = quantise_residual_block(&cur.y, w, x, y, &pred_y, 16, 0, 0, qs);
    let levels1 = quantise_residual_block(&cur.y, w, x + 8, y, &pred_y, 16, 8, 0, qs);
    let levels2 = quantise_residual_block(&cur.y, w, x, y + 8, &pred_y, 16, 0, 8, qs);
    let levels3 = quantise_residual_block(&cur.y, w, x + 8, y + 8, &pred_y, 16, 8, 8, qs);
    let levels4 = quantise_residual_block(&cur.cb, cw, x / 2, y / 2, &pred_cb, 8, 0, 0, qs);
    let levels5 = quantise_residual_block(&cur.cr, cw, x / 2, y / 2, &pred_cr, 8, 0, 0, qs);

    let mut cbp = 0u8;
    if levels0.iter().any(|&level| level != 0) {
        cbp |= 1;
    }
    if levels1.iter().any(|&level| level != 0) {
        cbp |= 2;
    }
    if levels2.iter().any(|&level| level != 0) {
        cbp |= 4;
    }
    if levels3.iter().any(|&level| level != 0) {
        cbp |= 8;
    }
    if levels4.iter().any(|&level| level != 0) {
        cbp |= 16;
    }
    if levels5.iter().any(|&level| level != 0) {
        cbp |= 32;
    }

    vlc::write_macroblock_address_increment(writer, 1);

    // Syntax order (13818-2 §6.2.5): macroblock_modes, motion_vectors, coded_block_pattern, blocks.
    if cbp != 0 {
        vlc::write_p_macroblock_type(writer, vlc::PType::McCoded);
    } else {
        vlc::write_p_macroblock_type(writer, vlc::PType::McNotCoded);
    }

    let delta_x = vlc::wrap_motion_delta(mv_x - *pmv_x, P_F_CODE);
    let delta_y = vlc::wrap_motion_delta(mv_y - *pmv_y, P_F_CODE);
    vlc::write_motion_delta(writer, delta_x, P_F_CODE);
    vlc::write_motion_delta(writer, delta_y, P_F_CODE);

    if cbp != 0 {
        // Internally bit 0 is block 0 (Y0); in the bitstream block 0 is the MSB of the 6-bit pattern.
        vlc::write_coded_block_pattern(writer, reverse_cbp(cbp));
    }

    *pmv_x = mv_x;
    *pmv_y = mv_y;

    if cbp & 1 != 0 {
        write_non_intra_block(writer, &levels0);
        reconstruct_inter_block(&mut recon.y, w, x, y, &pred_y, 16, 0, 0, Some(&levels0), qs);
    } else {
        reconstruct_inter_block(&mut recon.y, w, x, y, &pred_y, 16, 0, 0, None, qs);
    }

    if cbp & 2 != 0 {
        write_non_intra_block(writer, &levels1);
        reconstruct_inter_block(
            &mut recon.y,
            w,
            x + 8,
            y,
            &pred_y,
            16,
            8,
            0,
            Some(&levels1),
            qs,
        );
    } else {
        reconstruct_inter_block(&mut recon.y, w, x + 8, y, &pred_y, 16, 8, 0, None, qs);
    }

    if cbp & 4 != 0 {
        write_non_intra_block(writer, &levels2);
        reconstruct_inter_block(
            &mut recon.y,
            w,
            x,
            y + 8,
            &pred_y,
            16,
            0,
            8,
            Some(&levels2),
            qs,
        );
    } else {
        reconstruct_inter_block(&mut recon.y, w, x, y + 8, &pred_y, 16, 0, 8, None, qs);
    }

    if cbp & 8 != 0 {
        write_non_intra_block(writer, &levels3);
        reconstruct_inter_block(
            &mut recon.y,
            w,
            x + 8,
            y + 8,
            &pred_y,
            16,
            8,
            8,
            Some(&levels3),
            qs,
        );
    } else {
        reconstruct_inter_block(&mut recon.y, w, x + 8, y + 8, &pred_y, 16, 8, 8, None, qs);
    }

    if cbp & 16 != 0 {
        write_non_intra_block(writer, &levels4);
        reconstruct_inter_block(
            &mut recon.cb,
            cw,
            x / 2,
            y / 2,
            &pred_cb,
            8,
            0,
            0,
            Some(&levels4),
            qs,
        );
    } else {
        reconstruct_inter_block(&mut recon.cb, cw, x / 2, y / 2, &pred_cb, 8, 0, 0, None, qs);
    }

    if cbp & 32 != 0 {
        write_non_intra_block(writer, &levels5);
        reconstruct_inter_block(
            &mut recon.cr,
            cw,
            x / 2,
            y / 2,
            &pred_cr,
            8,
            0,
            0,
            Some(&levels5),
            qs,
        );
    } else {
        reconstruct_inter_block(&mut recon.cr, cw, x / 2, y / 2, &pred_cr, 8, 0, 0, None, qs);
    }

    *prev_intra = false;
}

fn block8x8(plane: &[u8], width: usize, x: usize, y: usize) -> [u8; 64] {
    let mut block = [0u8; 64];

    for row in 0..8 {
        for col in 0..8 {
            let index = (y + row).saturating_mul(width).saturating_add(x + col);
            if let Some(value) = plane.get(index).copied()
                && let Some(slot) = block.get_mut(row * 8 + col)
            {
                *slot = value;
            }
        }
    }

    block
}

fn copy_block8x8(plane: &mut [u8], width: usize, x: usize, y: usize, block: &[u8; 64]) {
    for row in 0..8 {
        for col in 0..8 {
            let index = (y + row).saturating_mul(width).saturating_add(x + col);
            if let Some(value) = block.get(row * 8 + col).copied()
                && let Some(slot) = plane.get_mut(index)
            {
                *slot = value;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)] // TODO: group into a context struct (tech debt)
fn residual_block(
    orig: &[u8],
    width: usize,
    x: usize,
    y: usize,
    pred: &[u8],
    pred_stride: usize,
    pred_x: usize,
    pred_y: usize,
) -> [i16; 64] {
    let mut residual = [0i16; 64];

    for row in 0..8 {
        for col in 0..8 {
            let orig_index = (y + row).saturating_mul(width).saturating_add(x + col);
            let pred_index = (pred_y + row)
                .saturating_mul(pred_stride)
                .saturating_add(pred_x + col);

            let o = i16::from(orig.get(orig_index).copied().unwrap_or(0));
            let p = i16::from(pred.get(pred_index).copied().unwrap_or(0));

            if let Some(slot) = residual.get_mut(row * 8 + col) {
                *slot = o - p;
            }
        }
    }

    residual
}

#[allow(clippy::too_many_arguments)] // TODO: group into a context struct (tech debt)
fn quantise_residual_block(
    orig: &[u8],
    width: usize,
    x: usize,
    y: usize,
    pred: &[u8],
    pred_stride: usize,
    pred_x: usize,
    pred_y: usize,
    qs: u16,
) -> [i16; 64] {
    let residual = residual_block(orig, width, x, y, pred, pred_stride, pred_x, pred_y);
    let coeffs = forward_dct8x8_i16(&residual);
    quant::quantise_non_intra(&coeffs, qs)
}

#[allow(clippy::too_many_arguments)] // TODO: group into a context struct (tech debt)
fn reconstruct_inter_block(
    recon_plane: &mut [u8],
    recon_width: usize,
    recon_x: usize,
    recon_y: usize,
    pred: &[u8],
    pred_stride: usize,
    pred_x: usize,
    pred_y: usize,
    levels: Option<&[i16; 64]>,
    qs: u16,
) {
    let residual = levels.map(|l| quant::reconstruct_residual(l, qs));

    for row in 0..8 {
        for col in 0..8 {
            let pred_index = (pred_y + row)
                .saturating_mul(pred_stride)
                .saturating_add(pred_x + col);
            let p = i32::from(pred.get(pred_index).copied().unwrap_or(0));

            let r = if let Some(ref res) = residual {
                res.get(row * 8 + col).copied().unwrap_or(0)
            } else {
                0
            };

            let value = (p + r).clamp(0, 255);
            let index = (recon_y + row)
                .saturating_mul(recon_width)
                .saturating_add(recon_x + col);

            if let Some(slot) = recon_plane.get_mut(index) {
                *slot = u8::try_from(value).unwrap_or(0);
            }
        }
    }
}

fn write_dc_diff(writer: &mut BitWriter, diff: i32, is_luma: bool) {
    let size = diff_size(diff);

    let code = if is_luma {
        vlc::dct_dc_size_luma(size)
    } else {
        vlc::dct_dc_size_chroma(size)
    };

    if let Some((bits, len)) = code {
        writer.write_bits(bits, len);
    }

    if size == 0 {
        return;
    }

    let abs = diff.unsigned_abs();
    let value = if diff > 0 {
        abs
    } else {
        (1u32 << u32::from(size)) - 1 - abs
    };

    writer.write_bits(value, size);
}

fn diff_size(diff: i32) -> u8 {
    if diff == 0 {
        return 0;
    }

    let mut value = diff.unsigned_abs();
    let mut size = 0u8;

    while value != 0 {
        size += 1;
        value >>= 1;
    }

    size
}

fn write_intra_ac(writer: &mut BitWriter, levels: &[i16; 64]) {
    let mut run = 0u32;

    for scan in 1..64 {
        let level = levels.get(scan).copied().unwrap_or(0);

        if level != 0 {
            vlc::write_escape_coefficient(
                writer,
                u8::try_from(run).unwrap_or(63),
                i32::from(level),
            );
            run = 0;
        } else {
            run += 1;
        }
    }

    vlc::write_end_of_block(writer);
}

fn write_non_intra_block(writer: &mut BitWriter, levels: &[i16; 64]) {
    let mut run = 0u32;

    for scan in 0..64 {
        let level = levels.get(scan).copied().unwrap_or(0);

        if level != 0 {
            vlc::write_escape_coefficient(
                writer,
                u8::try_from(run).unwrap_or(63),
                i32::from(level),
            );
            run = 0;
        } else {
            run += 1;
        }
    }

    vlc::write_end_of_block(writer);
}

fn build_cos_table() -> [[f64; 8]; 8] {
    let mut table = [[0.0f64; 8]; 8];

    for freq in 0..8 {
        for sample in 0..8 {
            let angle = (2 * sample + 1) as f64 * freq as f64 * std::f64::consts::PI / 16.0;
            if let Some(row) = table.get_mut(freq)
                && let Some(cell) = row.get_mut(sample)
            {
                *cell = angle.cos();
            }
        }
    }

    table
}

fn cos_table() -> &'static [[f64; 8]; 8] {
    COS_TABLE.get_or_init(build_cos_table)
}

fn forward_dct8x8_i16(samples: &[i16; 64]) -> [f64; 64] {
    let cos = cos_table();
    let mut temp = [0.0f64; 64];

    for row in 0..8 {
        for freq_h in 0..8 {
            let sum = (0..8).fold(0.0f64, |acc, x| {
                let sample = samples.get(row * 8 + x).copied().unwrap_or(0);
                let c = cos
                    .get(freq_h)
                    .and_then(|r| r.get(x))
                    .copied()
                    .unwrap_or(0.0);
                acc + f64::from(sample) * c
            });

            let scale = if freq_h == 0 { 0.5 * INV_SQRT2 } else { 0.5 };
            if let Some(slot) = temp.get_mut(row * 8 + freq_h) {
                *slot = sum * scale;
            }
        }
    }

    let mut out = [0.0f64; 64];

    for col in 0..8 {
        for freq_v in 0..8 {
            let sum = (0..8).fold(0.0f64, |acc, y| {
                let t = temp.get(y * 8 + col).copied().unwrap_or(0.0);
                let c = cos
                    .get(freq_v)
                    .and_then(|r| r.get(y))
                    .copied()
                    .unwrap_or(0.0);
                acc + t * c
            });

            let scale = if freq_v == 0 { 0.5 * INV_SQRT2 } else { 0.5 };
            if let Some(slot) = out.get_mut(freq_v * 8 + col) {
                *slot = sum * scale;
            }
        }
    }

    out
}
