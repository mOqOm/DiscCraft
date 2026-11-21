#![allow(dead_code)]

use crate::bitwriter::BitWriter;
use crate::dct;
use crate::motion;
use crate::quant;
use crate::vlc;
use crate::{EncodeError, Frame420, Standard};
use std::sync::OnceLock;

/// What depends on the picture format: MP@ML at a DVD's size, or MP@HL at 1920 x 1080.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Params {
    /// The coded size (whole macroblocks): the size of the frames the encoder is given.
    coded: (u16, u16),
    /// The size in the sequence header (what is shown).
    display: (u16, u16),
    /// The largest maximum bitrate, bits per second.
    max_bitrate: u32,
    /// The VBV buffer, bits (a multiple of 16,384).
    vbv_bits: u64,
    /// profile_and_level_indication.
    profile_level: u32,
    /// f_code of a P picture's forward vectors, both ways.
    f_code: u8,
    /// The motion search range in pixels (inside the f_code's range).
    search: i32,
}

impl Params {
    fn of(format: Format, standard: Standard) -> Self {
        match format {
            Format::Sd => Self {
                coded: (720, sd_height(standard)),
                display: (720, sd_height(standard)),
                max_bitrate: 9_800_000,
                vbv_bits: 1_835_008,
                profile_level: 0x48,
                f_code: 2,
                search: 15,
            },
            // MP@HL: a 9,781,248-bit VBV buffer; a Blu-ray's video is at most 40 Mbps. Vectors of +/- 63 pixels
            // (f_code 4 holds -128 to 127 half pixels).
            Format::Hd1080 => Self {
                coded: (1920, 1088),
                display: (1920, 1080),
                max_bitrate: 40_000_000,
                vbv_bits: 9_781_248,
                profile_level: 0x44,
                f_code: 4,
                search: 63,
            },
            // MP@H-14 (M6.393): HDV's 1440 x 1080, shown 16:9; a 7,340,032-bit VBV buffer (448 x 16,384), and at
            // most 40 Mbps as on a Blu-ray.
            Format::Hd1440 => Self {
                coded: (1440, 1088),
                display: (1440, 1080),
                max_bitrate: 40_000_000,
                vbv_bits: 7_340_032,
                profile_level: 0x46,
                f_code: 4,
                search: 63,
            },
            // MP@HL (M12.35): 1280 x 720 at 59.94 frames a second is more samples a second than High-1440 allows.
            Format::Hd720 => Self {
                coded: (1280, 720),
                display: (1280, 720),
                max_bitrate: 40_000_000,
                vbv_bits: 9_781_248,
                profile_level: 0x44,
                f_code: 4,
                search: 63,
            },
        }
    }

    /// The most luminance samples a second its level allows (13818-2 table 8-12): High's 62,668,800, High-1440's
    /// 47,001,600, Main's 10,368,000.
    fn samples_per_second(self) -> f64 {
        match self.profile_level {
            0x44 => 62_668_800.0,
            0x46 => 47_001_600.0,
            _ => 10_368_000.0,
        }
    }
}

fn sd_height(standard: Standard) -> u16 {
    match standard {
        Standard::Ntsc => 480,
        Standard::Pal => 576,
    }
}

/// The picture format of a sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// 720 x 480 (NTSC) or 720 x 576 (PAL), MP@ML: a DVD's video.
    #[default]
    Sd,
    /// 1920 x 1080 at the standard's frame rate (29.97 or 25 frames a second: 59.94i / 50i), MP@HL, BT.709,
    /// always 16:9: a Blu-ray's interlaced HD video. The frames given are 1920 x 1088 (the coded size: the last
    /// eight lines are not shown); each is coded as one frame picture of two fields from the same moment.
    Hd1080,
    /// 1440 x 1080 (pixels a third wider than tall, shown 16:9), MP@H-14, otherwise as [`Format::Hd1080`]: an HDV
    /// camcorder's size, so a Blu-ray title of HDV clips can have other clips encoded beside them (M6.393). The
    /// frames given are 1440 x 1088.
    Hd1440,
    /// 1280 x 720, MP@HL, BT.709, 16:9 (M12.35): a Blu-ray's 720p video, always a progressive sequence
    /// ([`Encoder::set_progressive`]). The frames given are 1280 x 720.
    Hd720,
}

/// The frame rate of a progressive sequence ([`Encoder::set_progressive`], M12.35): a Blu-ray's progressive HD
/// formats (1080p and 1440 x 1080p at 23.976 or 24, 720p at 23.976, 24, 50 or 59.94).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRate {
    /// 24000/1001.
    Fps23_976,
    /// 24.
    Fps24,
    /// 25.
    Fps25,
    /// 30000/1001.
    Fps29_97,
    /// 50.
    Fps50,
    /// 60000/1001.
    Fps59_94,
}

impl FrameRate {
    /// The rate of `numerator / denominator` frames a second, if it is one of these.
    #[must_use]
    pub fn of(numerator: u32, denominator: u32) -> Option<Self> {
        match (numerator, denominator) {
            (24_000, 1001) => Some(Self::Fps23_976),
            (24, 1) => Some(Self::Fps24),
            (25, 1) => Some(Self::Fps25),
            (30_000, 1001) => Some(Self::Fps29_97),
            (50, 1) => Some(Self::Fps50),
            (60_000, 1001) => Some(Self::Fps59_94),
            _ => None,
        }
    }

    /// frame_rate_code (13818-2 table 6-4).
    fn code(self) -> u32 {
        match self {
            Self::Fps23_976 => 1,
            Self::Fps24 => 2,
            Self::Fps25 => 3,
            Self::Fps29_97 => 4,
            Self::Fps50 => 6,
            Self::Fps59_94 => 7,
        }
    }

    /// The frames a second a time code counts (no drop frame): 24, 25, 30, 50 or 60.
    #[must_use]
    pub fn nominal(self) -> u32 {
        match self {
            Self::Fps23_976 | Self::Fps24 => 24,
            Self::Fps25 => 25,
            Self::Fps29_97 => 30,
            Self::Fps50 => 50,
            Self::Fps59_94 => 60,
        }
    }

    /// Seconds per frame.
    fn seconds(self) -> f64 {
        match self {
            Self::Fps23_976 => 1001.0 / 24_000.0,
            Self::Fps24 => 1.0 / 24.0,
            Self::Fps25 => 1.0 / 25.0,
            Self::Fps29_97 => 1001.0 / 30_000.0,
            Self::Fps50 => 1.0 / 50.0,
            Self::Fps59_94 => 1001.0 / 60_000.0,
        }
    }
}

impl Format {
    /// The size of the frames the encoder takes (whole macroblocks).
    #[must_use]
    pub fn coded_size(self, standard: Standard) -> (u16, u16) {
        Params::of(self, standard).coded
    }

    /// The VBV buffer in bits: the most one picture may take.
    #[must_use]
    pub fn vbv_bits(self) -> u64 {
        Params::of(self, Standard::Ntsc).vbv_bits
    }
}

const INV_SQRT2: f64 = 1.0 / std::f64::consts::SQRT_2;

/// The longest GOP a DVD allows, in frames: 18 for NTSC (36 fields), 15 for PAL (30 fields).
pub fn max_gop_frames(standard: Standard) -> u64 {
    match standard {
        Standard::Ntsc => 18,
        Standard::Pal => 15,
    }
}

/// The longest GOP of film pictures shown by 3:2 pulldown ([`EncoderConfig::pulldown`]) a DVD allows: 14 pictures
/// make at most 35 fields, within the 36 of an NTSC GOP.
pub const MAX_FILM_GOP_FRAMES: u64 = 14;

/// How film picture `index` (from 0) is shown by 3:2 pulldown: (top_field_first, repeat_first_field). Four pictures
/// make ten fields, T B T | B T | B T B | T B, so the fields keep alternating from one picture to the next.
#[must_use]
pub fn pulldown_flags(index: u64) -> (bool, bool) {
    match index % 4 {
        0 => (true, true),
        1 => (false, false),
        2 => (false, true),
        _ => (true, false),
    }
}

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
    /// Maximum bitrate in bits per second, at most 9,800,000 (SD) or 40,000,000 (HD).
    pub max_bitrate: u32,
    /// 16:9 display (aspect_ratio_information 3) instead of 4:3 (2). HD is always 16:9.
    pub wide: bool,
    /// The picture format.
    pub format: Format,
    /// Film (NTSC SD only; Encore's 23.976 presets): the frames given are 23.976 a second, progressive, and are
    /// shown by 3:2 pulldown ([`pulldown_flags`]) in the 29.97 sequence a DVD plays. A GOP holds at most
    /// [`MAX_FILM_GOP_FRAMES`] of them.
    pub pulldown: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PictureType {
    I,
    P,
    /// Predicted from the reference pictures before and after it in display order (never a reference itself).
    B,
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
    params: Params,
    frames_encoded: u64,
    /// Frames encoded since the current GOP started.
    frames_in_gop: u64,
    reference: Option<Frame420>,
    reconstruction: Option<Frame420>,
    quant_i: u8,
    quant_p: u8,
    vbv_fullness: u64,
    rate: RateControl,
    /// A second pass's plan (VBR 2 pass): each picture's bits from what the first pass measured.
    plan: Option<TwoPassPlan>,
    /// What the last coded picture took.
    last: Option<PictureStats>,
    /// How hard motion is searched for (Encore's quality, [`MIN_QUALITY`] to [`BEST_QUALITY`]).
    quality: u8,
    /// Interlaced pictures (M6.371): the field that comes first, `true` for the top one; `None` codes frames
    /// as progressive.
    interlaced: Option<bool>,
    /// With [`Encoder::interlaced`], whether macroblocks may also be coded as fields (dct_type); `false` keeps
    /// frame coding and only marks the pictures interlaced (M12.26).
    field_tools: bool,
    /// M12.36: the share of macroblocks that took a field tool in the last P and the last B picture coded with them
    /// since the GOP's I picture; `None` codes the next one with them, to measure.
    field_share: [Option<f64>; 2],
    /// M12.38: each frame coded as two field pictures ([`Encoder::set_field_pictures`]).
    field_pictures: bool,
    /// The last picture's vector of each macroblock (half pixels; empty after an I picture): where the search of
    /// the next picture's macroblock at the same place also starts (M6.394).
    vectors: Vec<(i32, i32)>,
    /// The reference picture before [`Encoder::reference`]: the past reference of the B pictures between them.
    older_reference: Option<Frame420>,
    /// B pictures between reference pictures ([`Encoder::set_b_frames`]; 0: I and P pictures only).
    b_frames: u8,
    /// Frames given to [`Encoder::push_frame`] that wait for the reference picture after them, with their display
    /// numbers.
    pending: Vec<(Frame420, u64)>,
    /// Frames given to [`Encoder::push_frame`] so far.
    pushed: u64,
    /// The display number of the current GOP's I picture ([`Encoder::push_frame`]).
    gop_first: u64,
    quant_b: u8,
    /// The temporal references of [`Encoder::reference`] and [`Encoder::older_reference`].
    reference_tr: u32,
    older_reference_tr: u32,
    /// The display number of the picture being coded (3:2 pulldown's fields and the GOP's time code go by it).
    coding_display: u64,
    /// The display number of the picture coded before it (the VBV buffer fills while that one is shown).
    coded_display: Option<u64>,
    /// A progressive sequence at this rate ([`Encoder::set_progressive`]); `None`: the standard's 29.97 or 25 frames
    /// a second, a sequence that may be interlaced.
    progressive: Option<FrameRate>,
}

/// The largest f_code: main profile's vertical vectors (13818-2 table 8-8).
const MAX_F_CODE: u8 = 5;

/// The most B pictures between two reference pictures.
pub const MAX_B_FRAMES: u8 = 2;

/// One coded picture of [`Encoder::push_frame`], in decoding order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodedPicture {
    /// The access unit: the picture, after the sequence and GOP headers when it starts a GOP.
    pub data: Vec<u8>,
    /// Its place in display order: how many frames were given before it.
    pub display: u64,
    /// What it took.
    pub stats: PictureStats,
    /// Whether it starts a (closed) GOP: an I picture after the sequence header.
    pub gop_start: bool,
    /// Whether it is an I or P picture, a reference of the pictures after it; `false` for a B picture.
    pub reference: bool,
}

/// The best quality (Encore's presets' 5, its "High quality"): the whole search range, to half a pixel.
pub const BEST_QUALITY: u8 = 5;

/// The lowest quality (Encore's quality slider's 1).
pub const MIN_QUALITY: u8 = 1;

/// The fewest pixels a lower quality still searches either way.
const LEAST_SEARCH: i32 = 4;

/// What one coded picture took: its bits, its quantiser_scale_code and whether it was an I picture. A first pass
/// of two records these for the second ([`Encoder::plan_second_pass`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PictureStats {
    /// The picture's bits.
    pub bits: u64,
    /// Its quantiser_scale_code (2–31).
    pub code: u8,
    /// An I picture.
    pub intra: bool,
    /// A B picture.
    pub bidirectional: bool,
}

/// How much of a picture's complexity its bits follow in a second pass (0: the same bits for every picture, 1:
/// the same quantiser): between the two, so a hard scene gets more bits and an easy one gives some up, as
/// two-pass encoders commonly do.
const PLAN_COMPRESSION: f64 = 0.6;

/// How many pictures either side a first-pass picture's complexity is averaged with (those of its own type): one
/// picture's bits at one quantiser are a noisy measure, its neighbours' smooth it.
const PLAN_BLUR: usize = 7;

/// How much of the correction ([`TwoPassPlan::scale`]) the newest picture moves.
const SCALE_STEP: f64 = 0.2;

/// A second pass's bits for each picture (VBR 2 pass): the sequence's bits shared in proportion to each picture's
/// complexity (first-pass bits × quantiser, averaged with its neighbours of the same type) raised to
/// [`PLAN_COMPRESSION`], none more than it takes at the finest quantiser (the rest goes to the others); the bits
/// spent beyond the plan so far are paid back over the next two GOPs.
#[derive(Debug, Clone, PartialEq)]
struct TwoPassPlan {
    complexity: Vec<f64>,
    target: Vec<f64>,
    debt: f64,
    /// How many times its foretold bits (complexity over quantiser) a picture of this pass took, lately: coded
    /// finer than in the first pass, a picture takes more than its complexity says. Pictures at the finest
    /// quantiser are left out (their bits do not follow it).
    scale: f64,
}

impl TwoPassPlan {
    fn new(first: &[PictureStats], frame_bits: f64) -> Self {
        let raw: Vec<f64> = first
            .iter()
            .map(|picture| (picture.bits as f64 * f64::from(picture.code)).max(1.0))
            .collect();
        let complexity: Vec<f64> = first
            .iter()
            .enumerate()
            .map(|(index, picture)| {
                let from = index.saturating_sub(PLAN_BLUR);
                let (sum, count) = first
                    .iter()
                    .zip(&raw)
                    .enumerate()
                    .skip(from)
                    .take(2 * PLAN_BLUR + 1)
                    .filter(|(_, (other, _))| {
                        (other.intra, other.bidirectional) == (picture.intra, picture.bidirectional)
                    })
                    .fold((0.0, 0.0), |(sum, count), (_, (_, value))| {
                        (sum + value, count + 1.0)
                    });
                if count > 0.0 { sum / count } else { 1.0 }
            })
            .collect();
        let weights: Vec<f64> = complexity
            .iter()
            .map(|complexity| complexity.powf(PLAN_COMPRESSION))
            .collect();
        // At most what a picture takes at the finest quantiser (2); what that leaves goes to the others.
        let caps: Vec<f64> = complexity
            .iter()
            .map(|complexity| complexity / 2.0)
            .collect();
        // `as` is exact for a sequence's count of pictures.
        let total = frame_bits * first.len() as f64;
        let mut target = vec![frame_bits; first.len()];
        let mut capped = vec![false; first.len()];
        for _ in 0..8 {
            let spent: f64 = target
                .iter()
                .zip(&capped)
                .filter(|(_, capped)| **capped)
                .map(|(target, _)| *target)
                .sum();
            let free: f64 = weights
                .iter()
                .zip(&capped)
                .filter(|(_, capped)| !**capped)
                .map(|(weight, _)| *weight)
                .sum();
            if !(free > 0.0 && free.is_finite()) {
                break;
            }
            let share = (total - spent).max(0.0) / free;
            let mut changed = false;
            for (((target, capped), weight), cap) in target
                .iter_mut()
                .zip(capped.iter_mut())
                .zip(&weights)
                .zip(&caps)
            {
                if *capped {
                    continue;
                }
                *target = share * weight;
                if *target > *cap {
                    *target = *cap;
                    *capped = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        Self {
            complexity,
            target,
            debt: 0.0,
            scale: 1.0,
        }
    }

    /// The quantiser_scale_code for picture `index`, aiming at its planned bits less its share of the debt and
    /// leaving the VBV margin free of the `available` bits; `None` past the plan.
    fn code(&self, index: usize, budget: Budget, available: u64) -> Option<u8> {
        let complexity = *self.complexity.get(index)?;
        let target = *self.target.get(index)?;
        let payback = self.debt / (2.0 * budget.gop_frames.max(1.0));
        let aimed = (target - payback).max(target * MIN_TARGET_SHARE);
        let available = available as f64;
        let room = (available - VBV_MARGIN * budget.vbv_bits).max(available / 2.0);
        let aimed = aimed.min(room).max(1.0);
        Some((complexity * self.scale / aimed).round().clamp(2.0, 31.0) as u8)
    }

    /// Notes that picture `index` took `bits` at `code`.
    fn spent(&mut self, index: usize, bits: u64, code: u8) {
        if let Some(target) = self.target.get(index) {
            self.debt += bits as f64 - target;
        }
        if let Some(complexity) = self.complexity.get(index).filter(|_| code > 2) {
            let foretold = complexity / f64::from(code);
            if foretold > 0.0 {
                let ratio = (bits as f64 / foretold).clamp(0.25, 4.0);
                self.scale =
                    (self.scale * (1.0 - SCALE_STEP) + ratio * SCALE_STEP).clamp(0.25, 4.0);
            }
        }
    }
}

/// Picture-level rate control in the manner of MPEG-2 Test Model 5: each picture type has a complexity
/// (bits × quantiser_scale_code, so a picture coded at code `q` is expected to take `complexity / q` bits); a nominal
/// GOP's bits are shared between its I picture and its P pictures in proportion to their complexities; and the bits
/// spent beyond the average so far are paid back over the next two GOPs, so the average converges on the target.
#[derive(Debug, Clone, Default)]
struct RateControl {
    complexity_i: Option<f64>,
    complexity_p: Option<f64>,
    complexity_b: Option<f64>,
    /// Bits spent beyond the average budget so far (negative when under it).
    debt: f64,
}

/// The quantiser_scale_code of the first picture, before any complexity is known.
const FIRST_CODE: u8 = 6;
/// A P picture takes about this share of an I picture's bits at the same quantiser, until one has been coded.
const P_TO_I_COMPLEXITY: f64 = 0.35;
/// How far the quantiser of a picture type may fall from the last one of its type (it may rise any amount), so one
/// nearly empty picture (a still, a repeated frame) does not make the next real one far too large.
const MAX_CODE_FALL: u8 = 3;
/// How much coarser a P picture is quantised than an I picture (Test Model 5's K_p): the I picture is the
/// reference the GOP's P pictures are predicted from, so its quality carries.
const P_TO_I_CODE: f64 = 1.25;
/// A B picture takes about this share of an I picture's bits at the same quantiser, until one has been coded.
const B_TO_I_COMPLEXITY: f64 = 0.2;
/// How much coarser a B picture is quantised than an I picture (Test Model 5's K_b of 1.4 over a P picture's): no
/// picture is predicted from it.
const B_TO_I_CODE: f64 = P_TO_I_CODE * 1.4;
/// The smallest share of its nominal budget a picture gets however large the debt.
const MIN_TARGET_SHARE: f64 = 0.25;
/// The VBV buffer a picture's target leaves free, as a share of the buffer.
const VBV_MARGIN: f64 = 0.1;

impl RateControl {
    /// The quantiser_scale_code (2–31) for the next picture of `picture_type`, aiming at its share of the budget
    /// and leaving the VBV margin free of the `available` bits. `codes` are the last I, P and B pictures' codes.
    fn code(
        &self,
        picture_type: PictureType,
        budget: Budget,
        available: u64,
        codes: (u8, u8, u8),
    ) -> u8 {
        let Some(complexity_i) = self.complexity_i else {
            return FIRST_CODE;
        };
        let complexity_p = self
            .complexity_p
            .unwrap_or(complexity_i * P_TO_I_COMPLEXITY);
        let complexity_b = self
            .complexity_b
            .unwrap_or(complexity_i * B_TO_I_COMPLEXITY);
        // Shares in which a P picture's code comes out P_TO_I_CODE times the I picture's, a B picture's B_TO_I_CODE.
        let (weight_i, weight_p, weight_b) = (
            complexity_i,
            complexity_p / P_TO_I_CODE,
            complexity_b / B_TO_I_CODE,
        );
        let (count_p, count_b) = budget.inter_counts();
        let total = weight_i + count_p * weight_p + count_b * weight_b;
        let floor_of = |ratio: f64| (f64::from(codes.0) * ratio).round().clamp(2.0, 31.0) as u8;
        let (complexity, share, last, floor) = match picture_type {
            PictureType::I => (complexity_i, weight_i / total, codes.0, 2),
            PictureType::P => (
                complexity_p,
                weight_p / total,
                codes.1,
                floor_of(P_TO_I_CODE),
            ),
            PictureType::B => (
                complexity_b,
                weight_b / total,
                codes.2,
                floor_of(B_TO_I_CODE),
            ),
        };
        let nominal = budget.frame_bits * budget.gop_frames * share;
        let target = (nominal - self.debt * share / 2.0).max(nominal * MIN_TARGET_SHARE);
        let available = available as f64;
        let room = (available - VBV_MARGIN * budget.vbv_bits).max(available / 2.0);
        let target = target.min(room).max(1.0);
        let code = (complexity / target).round().clamp(2.0, 31.0) as u8;
        code.max(last.saturating_sub(MAX_CODE_FALL)).max(floor)
    }

    /// Learns from a picture of `picture_type` coded in `bits` at `code`.
    fn update(&mut self, picture_type: PictureType, bits: u64, code: u8, budget: Budget) {
        let measured = bits as f64 * f64::from(code);
        let slot = match picture_type {
            PictureType::I => &mut self.complexity_i,
            PictureType::P => &mut self.complexity_p,
            PictureType::B => &mut self.complexity_b,
        };
        *slot = Some(slot.map_or(measured, |previous| (previous + measured) / 2.0));
        // Savings are kept for at most two GOPs, so a long still does not buy one burst much later.
        let gop_bits = budget.frame_bits * budget.gop_frames;
        self.debt = (self.debt + bits as f64 - budget.frame_bits).max(-2.0 * gop_bits);
    }
}

/// The average bits per picture and the nominal GOP length, in pictures.
#[derive(Debug, Clone, Copy)]
struct Budget {
    frame_bits: f64,
    gop_frames: f64,
    /// The VBV buffer, bits.
    vbv_bits: f64,
    /// B pictures between reference pictures.
    b_frames: u8,
}

impl Budget {
    /// How many P and B pictures a nominal GOP has besides its I picture: a P picture after every `b_frames` B
    /// pictures.
    fn inter_counts(self) -> (f64, f64) {
        let rest = (self.gop_frames - 1.0).max(0.0);
        let count_p = (rest / (f64::from(self.b_frames) + 1.0)).ceil();
        (count_p, rest - count_p)
    }
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

        if config.pulldown && (config.standard != Standard::Ntsc || config.format != Format::Sd) {
            return Err(EncodeError(
                "3:2 pulldown is for an NTSC DVD's video only".to_string(),
            ));
        }

        let params = Params::of(config.format, config.standard);
        if config.max_bitrate > params.max_bitrate {
            return Err(EncodeError(format!(
                "max_bitrate must be <= {}",
                params.max_bitrate
            )));
        }

        Ok(Self {
            config,
            params,
            frames_encoded: 0,
            frames_in_gop: 0,
            reference: None,
            reconstruction: None,
            quant_i: FIRST_CODE,
            quant_p: FIRST_CODE,
            vbv_fullness: params.vbv_bits,
            rate: RateControl::default(),
            plan: None,
            last: None,
            quality: BEST_QUALITY,
            interlaced: None,
            field_tools: false,
            field_share: [None; 2],
            field_pictures: false,
            vectors: Vec::new(),
            older_reference: None,
            b_frames: 0,
            pending: Vec::new(),
            pushed: 0,
            gop_first: 0,
            quant_b: FIRST_CODE,
            reference_tr: 0,
            older_reference_tr: 0,
            coding_display: 0,
            coded_display: None,
            progressive: None,
        })
    }

    /// Makes the sequence progressive at `rate` (M12.35: a Blu-ray's 1080p, 1440 x 1080p and 720p): progressive_sequence
    /// 1, the rate's frame_rate_code, and every picture a progressive frame shown once (top_field_first and
    /// repeat_first_field 0). Its GOPs are at most a second long. An interlaced setting before it is dropped.
    ///
    /// # Errors
    ///
    /// For a DVD's format, with 3:2 pulldown, or when the format's level does not allow that many samples a second
    /// (1080 lines at 50 or 59.94 frames).
    pub fn set_progressive(&mut self, rate: FrameRate) -> Result<(), EncodeError> {
        if self.config.format == Format::Sd || self.config.pulldown {
            return Err(EncodeError(
                "a progressive sequence is for an HD format without pulldown".to_string(),
            ));
        }
        let (width, height) = self.params.coded;
        if f64::from(width) * f64::from(height) / rate.seconds() > self.params.samples_per_second()
        {
            return Err(EncodeError(format!(
                "{width} x {height} at {} frames a second is more than the level allows",
                rate.nominal()
            )));
        }
        self.progressive = Some(rate);
        self.interlaced = None;
        self.field_tools = false;
        self.field_pictures = false;
        Ok(())
    }

    /// Sets how hard motion is searched for, as Encore's quality (its "Low quality" presets' 2, "Long Form"
    /// MPEG-2's 3, the others' 5; clamped to [`MIN_QUALITY`]..=[`BEST_QUALITY`]): a lower one searches a smaller
    /// range (the full one times quality / 5, at least 4 pixels) and below 3 to whole pixels only. Faster, and
    /// the pictures are a little worse at the same bit rate.
    pub fn set_quality(&mut self, quality: u8) {
        self.quality = quality.clamp(MIN_QUALITY, BEST_QUALITY);
    }

    /// Codes the pictures as interlaced (M6.371): frames whose two fields were taken at different times, the top
    /// one first when `top_first`, and the pictures say they are not progressive. Each macroblock is transformed as
    /// a frame or as two fields, whichever its levels are cheaper (dct_type), and (M12.36) a P or B picture's
    /// macroblock is predicted as a frame or each of its fields from either field of a reference (field
    /// prediction), whichever is clearly closer. A P or B picture keeps these tools while enough macroblocks of the
    /// last picture of its type took them (they cost bits on every macroblock); each GOP measures again. Not with
    /// 3:2 pulldown (its pictures are progressive frames). The DVD transcodes use it for an interlaced source.
    pub fn set_interlaced(&mut self, top_first: bool) {
        self.interlaced = Some(top_first);
        self.field_tools = true;
        self.field_pictures = false;
    }

    /// Codes each frame as two field pictures (M12.38, 13818-2 6.1.1.4): the field that comes first (the top one
    /// when `top_first`), then the other, under one temporal_reference. An I frame's second field is a P field
    /// predicted from its first; a P frame's fields are P fields predicted from either field of the reference frame
    /// (the second also from the first); a B frame's fields are B fields predicted from the fields of the two
    /// reference frames. Each macroblock is 16 x 16 in its field, with one vector a direction and the field it
    /// comes from (field_motion_type 01). Not with 3:2 pulldown or a progressive sequence. The transcodes do not use
    /// it: it makes the field-picture streams some broadcast and recorder video carries, to test reading them.
    pub fn set_field_pictures(&mut self, top_first: bool) {
        self.interlaced = Some(top_first);
        self.field_tools = false;
        self.field_pictures = true;
    }

    /// Marks the pictures interlaced, the top field first when `top_first` (M12.26): progressive_frame 0,
    /// top_field_first as given, chroma 4:2:0 of fields. The macroblocks are coded as frames, as for progressive
    /// pictures (the DVD transcodes used this before the field prediction of [`Encoder::set_interlaced`],
    /// M12.36). Not with 3:2 pulldown.
    pub fn set_field_order(&mut self, top_first: bool) {
        self.interlaced = Some(top_first);
        self.field_tools = false;
        self.field_pictures = false;
    }

    /// The f_code and search range (pixels, from `search` for one picture's distance) of a picture of
    /// `picture_type`: with B pictures a P picture is `b_frames + 1` pictures from its reference and a B picture up
    /// to `b_frames`, so motion reaches that much further. The f_code is at most 5 (the vertical vectors of main
    /// profile, 13818-2 table 8-8), the search inside its range.
    fn reach(&self, picture_type: PictureType, search: i32) -> (u8, i32) {
        let distance: i32 = match (picture_type, self.b_frames) {
            (_, 0) | (PictureType::I, _) => 1,
            (PictureType::P, b) => i32::from(b) + 1,
            (PictureType::B, b) => i32::from(b),
        };
        // f_code f reaches 8 << (f - 1) pixels; one step more for each doubling of the distance.
        let steps = u8::try_from(32 - (distance - 1).max(0).leading_zeros()).unwrap_or(0);
        let f_code = self.params.f_code.saturating_add(steps).min(MAX_F_CODE);
        let reach = (8i32 << (f_code - 1)) - 1;
        (f_code, search.saturating_mul(distance).min(reach))
    }

    /// The motion search range and whether vectors are refined to half a pixel, at the encoder's quality.
    fn search(&self) -> (i32, bool) {
        let full = self.params.search;
        if self.quality >= BEST_QUALITY {
            return (full, true);
        }
        let range = (full * i32::from(self.quality) / i32::from(BEST_QUALITY)).max(LEAST_SEARCH);
        (range.min(full), self.quality >= 3)
    }

    /// Makes this encoder the second pass of two (VBR 2 pass): `first` is what a first pass over the same pictures
    /// with the same configuration measured, one entry a picture ([`Encoder::last_picture`]). Each picture then
    /// gets its share of the sequence's bits by its complexity; pictures past the plan fall back to the one-pass
    /// rate control.
    pub fn plan_second_pass(&mut self, first: &[PictureStats]) {
        let frame_bits = self.budget().frame_bits;
        self.plan = Some(TwoPassPlan::new(first, frame_bits));
    }

    /// What the last coded picture took (a first pass records it for each picture).
    #[must_use]
    pub fn last_picture(&self) -> Option<PictureStats> {
        self.last
    }

    /// Encode one frame as one access unit, a closed GOP starting every `gop_size` frames.
    pub fn encode_frame(&mut self, frame: &Frame420) -> Result<Vec<u8>, EncodeError> {
        let start = self.frames_in_gop >= u64::from(self.config.gop_size);
        self.encode_frame_gop(frame, start)
    }

    /// Encode one frame as one access unit, starting a new closed GOP with it when `start_gop` (a chapter or a
    /// layer break, where a DVD cell must begin exactly). The first frame always starts one, and so does a frame
    /// that would make the GOP longer than a DVD allows (18 frames NTSC, 15 PAL); otherwise the GOP goes on, past
    /// `gop_size` too, as the caller decides.
    pub fn encode_frame_gop(
        &mut self,
        frame: &Frame420,
        start_gop: bool,
    ) -> Result<Vec<u8>, EncodeError> {
        validate_frame(frame, self.params.coded)?;

        let is_gop_start =
            start_gop || self.frames_encoded == 0 || self.frames_in_gop >= self.max_gop();
        if is_gop_start {
            self.frames_in_gop = 0;
        }
        let picture_type = if is_gop_start {
            PictureType::I
        } else {
            PictureType::P
        };
        // At most `max_gop_frames`, so it fits the 10-bit field.
        let temporal_reference = u32::try_from(self.frames_in_gop).unwrap_or(0);
        let display = self.frames_encoded;
        let data = self.code_picture(
            frame,
            picture_type,
            is_gop_start,
            temporal_reference,
            display,
        )?;
        self.frames_in_gop += 1;
        Ok(data)
    }

    /// Sets how many B pictures [`Encoder::push_frame`] puts between reference pictures (0 to [`MAX_B_FRAMES`]; 0,
    /// the default, codes I and P pictures only). With 3:2 pulldown each picture repeats a field by its place in
    /// display order, and the VBV buffer fills while the picture coded before it is shown (M12.34).
    pub fn set_b_frames(&mut self, count: u8) {
        self.b_frames = count.min(MAX_B_FRAMES);
    }

    /// Gives the next frame in display order and returns the pictures that can be coded now, in decoding order:
    /// with B pictures ([`Encoder::set_b_frames`]) a frame waits until the reference picture after it is coded, so
    /// a call returns none, or a P picture followed by the B pictures before it. `start_gop` starts a new closed
    /// GOP with this frame (an I picture), as [`Encoder::encode_frame_gop`]; the frames still waiting are coded
    /// first, the last of them as a P picture so the GOP stays closed. [`Encoder::flush`] codes what waits at the end.
    ///
    /// # Errors
    ///
    /// As [`Encoder::encode_frame_gop`].
    pub fn push_frame(
        &mut self,
        frame: &Frame420,
        start_gop: bool,
    ) -> Result<Vec<CodedPicture>, EncodeError> {
        validate_frame(frame, self.params.coded)?;
        let display = self.pushed;
        self.pushed = self.pushed.saturating_add(1);
        if self.b_frames == 0 {
            let gop_start =
                start_gop || self.frames_encoded == 0 || self.frames_in_gop >= self.max_gop();
            let data = self.encode_frame_gop(frame, start_gop)?;
            let stats = self.last.unwrap_or(PictureStats {
                bits: 0,
                code: 0,
                intra: gop_start,
                bidirectional: false,
            });
            return Ok(vec![CodedPicture {
                data,
                display,
                stats,
                gop_start,
                reference: true,
            }]);
        }
        let in_gop = display.saturating_sub(self.gop_first);
        if start_gop || display == 0 || in_gop >= self.max_gop() {
            let mut out = self.flush()?;
            self.gop_first = display;
            self.frames_in_gop = 0;
            let data = self.code_picture(frame, PictureType::I, true, 0, display)?;
            out.push(self.coded(data, display, PictureType::I, true));
            return Ok(out);
        }
        self.pending.push((frame.clone(), display));
        if self.pending.len() > usize::from(self.b_frames) {
            return self.flush();
        }
        Ok(Vec::new())
    }

    /// Codes the frames that wait for a reference picture ([`Encoder::push_frame`]): the last of them as a P
    /// picture, then the ones before it as B pictures between the reference before them and it. Call it after the
    /// last frame.
    ///
    /// # Errors
    ///
    /// As [`Encoder::encode_frame_gop`].
    pub fn flush(&mut self) -> Result<Vec<CodedPicture>, EncodeError> {
        let mut pending = std::mem::take(&mut self.pending);
        let Some((last, last_display)) = pending.pop() else {
            return Ok(Vec::new());
        };
        let first = self.gop_first;
        // At most `max_gop_frames` after the GOP's I picture, so it fits the 10-bit field.
        let temporal = |display: u64| u32::try_from(display.saturating_sub(first)).unwrap_or(0);
        let mut out = Vec::with_capacity(pending.len().saturating_add(1));
        let data = self.code_picture(
            &last,
            PictureType::P,
            false,
            temporal(last_display),
            last_display,
        )?;
        out.push(self.coded(data, last_display, PictureType::P, false));
        for (frame, display) in pending {
            let data =
                self.code_picture(&frame, PictureType::B, false, temporal(display), display)?;
            out.push(self.coded(data, display, PictureType::B, false));
        }
        Ok(out)
    }

    /// The [`CodedPicture`] of the picture just coded.
    fn coded(
        &self,
        data: Vec<u8>,
        display: u64,
        picture_type: PictureType,
        gop_start: bool,
    ) -> CodedPicture {
        CodedPicture {
            data,
            display,
            stats: self.last.unwrap_or(PictureStats {
                bits: 0,
                code: 0,
                intra: picture_type == PictureType::I,
                bidirectional: picture_type == PictureType::B,
            }),
            gop_start,
            reference: picture_type != PictureType::B,
        }
    }

    /// Codes `frame` as one picture of `picture_type` at the rate control's quantiser (coarser until it fits the VBV
    /// buffer), notes what it took, and keeps an I or P picture's reconstruction as the reference.
    fn code_picture(
        &mut self,
        frame: &Frame420,
        picture_type: PictureType,
        is_gop_start: bool,
        temporal_reference: u32,
        display: u64,
    ) -> Result<Vec<u8>, EncodeError> {
        self.coding_display = display;
        let fill = self.fill_bits();
        let fullness = self
            .vbv_fullness
            .saturating_add(fill)
            .min(self.params.vbv_bits);
        let budget = self.budget();
        let codes = (self.quant_i, self.quant_p, self.quant_b);
        let index = usize::try_from(self.frames_encoded).unwrap_or(usize::MAX);
        let mut code = match self
            .plan
            .as_ref()
            .and_then(|plan| plan.code(index, budget, fullness))
        {
            Some(code) => code,
            None => self.rate.code(picture_type, budget, fullness, codes),
        };
        let mut attempts = 0u32;
        let mut bits;

        let (data, recon, vectors, uses) = loop {
            let encoded =
                self.encode_picture(frame, picture_type, is_gop_start, temporal_reference, code)?;

            bits = u64::try_from(encoded.0.len())
                .unwrap_or(u64::MAX)
                .saturating_mul(8);

            if bits <= fullness {
                break encoded;
            }

            // Coarser and coarser, up to the coarsest quantiser: only a picture that does not fit even there fails.
            if code >= 31 {
                return Err(EncodeError(format!(
                    "VBV underflow: {bits} bits at quantiser 31 after {attempts} retries, {fullness} available"
                )));
            }

            attempts += 1;
            code = code.saturating_add(4).min(31);
        };

        self.vbv_fullness = fullness.saturating_sub(bits);

        self.rate.update(picture_type, bits, code, budget);
        if let Some(plan) = self.plan.as_mut() {
            plan.spent(index, bits, code);
        }
        self.last = Some(PictureStats {
            bits,
            code,
            intra: picture_type == PictureType::I,
            bidirectional: picture_type == PictureType::B,
        });
        match picture_type {
            PictureType::I => self.quant_i = code,
            PictureType::P => self.quant_p = code,
            PictureType::B => self.quant_b = code,
        }

        self.frames_encoded += 1;
        self.coded_display = Some(display);
        match (picture_type, uses.share()) {
            (PictureType::I, _) => self.field_share = [None; 2],
            (PictureType::P, Some(share)) => self.field_share[0] = Some(share),
            (PictureType::B, Some(share)) => self.field_share[1] = Some(share),
            _ => {}
        }
        if picture_type != PictureType::B {
            // A reference picture: the B pictures after it in decoding order look back to the one before it.
            self.older_reference = self.reference.replace(recon.clone());
            self.older_reference_tr = self.reference_tr;
            self.reference_tr = temporal_reference;
            self.vectors = vectors;
        }
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

    /// The quantiser_scale_codes (2–31; larger is coarser) of the last I and P pictures.
    pub fn quantiser_codes(&self) -> (u8, u8) {
        (self.quant_i, self.quant_p)
    }

    /// Number of frames encoded so far.
    pub fn frames_encoded(&self) -> u64 {
        self.frames_encoded
    }

    fn fps(&self) -> u32 {
        if let Some(rate) = self.progressive {
            return rate.nominal();
        }
        match self.config.standard {
            Standard::Ntsc => 30,
            Standard::Pal => 25,
        }
    }

    /// The longest GOP, in pictures: a progressive sequence's a second.
    fn max_gop(&self) -> u64 {
        if let Some(rate) = self.progressive {
            u64::from(rate.nominal())
        } else if self.config.pulldown {
            MAX_FILM_GOP_FRAMES
        } else {
            max_gop_frames(self.config.standard)
        }
    }

    /// Seconds per picture: 1001/30000 (NTSC) or 1/25 (PAL); film's 1001/24000 on average (two and a half fields);
    /// a progressive sequence's own.
    fn frame_seconds(&self) -> f64 {
        if let Some(rate) = self.progressive {
            return rate.seconds();
        }
        match (self.config.standard, self.config.pulldown) {
            (Standard::Ntsc, true) => 1001.0 / 24000.0,
            (Standard::Ntsc, false) => 1001.0 / 30000.0,
            (Standard::Pal, _) => 1.0 / 25.0,
        }
    }

    /// The fields picture `index` is shown for: two, or three when pulldown repeats its first field.
    fn picture_fields(&self, index: u64) -> u64 {
        if self.config.pulldown && pulldown_flags(index).1 {
            3
        } else {
            2
        }
    }

    /// The pictures shown before picture `index`, counted in the sequence's frames (29.97 or 25 a second): film's
    /// shown fields, two a frame.
    fn shown_frames_before(&self, index: u64) -> u64 {
        if self.config.pulldown {
            // Ten fields every four pictures, then those of the pictures past the last four.
            let whole = index / 4 * 10;
            let rest: u64 = (index / 4 * 4..index)
                .map(|picture| self.picture_fields(picture))
                .sum();
            (whole + rest) / 2
        } else {
            index
        }
    }

    fn budget(&self) -> Budget {
        Budget {
            frame_bits: f64::from(self.config.bitrate) * self.frame_seconds(),
            gop_frames: f64::from(self.config.gop_size.max(1)),
            vbv_bits: self.params.vbv_bits as f64,
            b_frames: self.b_frames,
        }
    }

    /// The bits the VBV buffer gains in one picture period, filling at the maximum bitrate: film's picture coded
    /// before this one was shown for two or three fields (decoding goes on at the pace the pictures are coded in,
    /// each taking its own fields, as the multiplexer times them).
    fn fill_bits(&self) -> u64 {
        let seconds = if self.config.pulldown {
            let before = self.coded_display.unwrap_or(0);
            self.picture_fields(before) as f64 * 1001.0 / 60000.0
        } else {
            self.frame_seconds()
        };
        ((f64::from(self.config.max_bitrate) * seconds) as u64).max(1)
    }

    fn encode_picture(
        &self,
        frame: &Frame420,
        picture_type: PictureType,
        is_gop_start: bool,
        temporal_reference: u32,
        code: u8,
    ) -> Result<(Vec<u8>, Frame420, Vec<(i32, i32)>, FieldUse), EncodeError> {
        let mut writer = BitWriter::new();
        let mut recon = empty_frame(frame.width, frame.height);

        if is_gop_start {
            self.write_sequence_headers(&mut writer);
            write_gop_header(
                &mut writer,
                self.shown_frames_before(self.coding_display),
                self.fps(),
            );
        }

        if let Some(top_first) = self
            .interlaced
            .filter(|_| self.field_pictures && !self.config.pulldown)
        {
            return self.encode_field_pair(
                writer,
                frame,
                picture_type,
                (temporal_reference, code),
                top_first,
            );
        }
        write_picture_header(&mut writer, picture_type, temporal_reference);
        let field = self.interlaced.filter(|_| !self.config.pulldown);
        let fields = if self.config.pulldown {
            pulldown_flags(self.coding_display)
        } else if self.progressive.is_some() {
            // In a progressive sequence, a frame shown once (13818-2 6.3.10).
            (false, false)
        } else {
            (field.unwrap_or(true), false)
        };
        // M12.36: the field tools put frame_motion_type and dct_type on every coded macroblock of a P or B picture;
        // they are kept while enough of the macroblocks of the last picture of the type took them (each GOP measures
        // again).
        let share = match picture_type {
            PictureType::I => None,
            PictureType::P => self.field_share[0].map(|share| (share, FIELD_TOOLS_SHARE_P)),
            PictureType::B => self.field_share[1].map(|share| (share, FIELD_TOOLS_SHARE_B)),
        };
        let field_tools = field.is_some()
            && self.field_tools
            && share.is_none_or(|(share, least)| share >= least);
        write_picture_coding_extension(
            &mut writer,
            picture_type,
            self.reach(picture_type, self.search().0).0,
            fields,
            (field.is_some(), field_tools),
        );

        let qs = u16::from(code).saturating_mul(2);

        let missing = || EncodeError("missing reference frame".to_string());
        // An interlaced picture with the field tools also predicts each field of a macroblock from either field of a
        // reference (M12.36): the fields of the picture and of its references, as pictures of their own.
        let fields_of = |picture: &Frame420| [motion::field(picture, 0), motion::field(picture, 1)];
        let field_set = (picture_type != PictureType::I && field_tools)
            .then(|| {
                let (past, future) = match picture_type {
                    PictureType::B => (self.older_reference.as_ref(), self.reference.as_ref()),
                    _ => (self.reference.as_ref(), None),
                };
                past.map(|past| FieldSet {
                    current: fields_of(frame),
                    past: fields_of(past),
                    future: future.map(fields_of),
                })
            })
            .flatten();
        let refs = match picture_type {
            PictureType::I => Refs::None,
            PictureType::P => Refs::Forward(
                self.reference.as_ref().ok_or_else(missing)?,
                field_set.as_ref(),
            ),
            PictureType::B => Refs::Both(
                self.older_reference.as_ref().ok_or_else(missing)?,
                self.reference.as_ref().ok_or_else(missing)?,
                field_set.as_ref(),
            ),
        };
        // A P picture's searches start from the last reference's vectors; a B picture's from the future reference's,
        // scaled to its place between the two.
        let previous: &[(i32, i32)] = &self.vectors;
        let mut data = writer.finish();
        let workers = slice_workers(usize::from(frame.height) / 16);
        let (search, half_pel) = self.search();
        let (f_code, search) = self.reach(picture_type, search);
        let coding = Coding {
            code,
            qs,
            f_code,
            search,
            half_pel,
            field: field_tools,
            between: (
                i32::try_from(temporal_reference.saturating_sub(self.older_reference_tr))
                    .unwrap_or(0),
                i32::try_from(self.reference_tr.saturating_sub(self.older_reference_tr))
                    .unwrap_or(0),
            ),
        };
        let (vectors, uses) = encode_slices(
            frame,
            (refs, previous),
            &mut recon,
            (coding, workers),
            &mut data,
        )?;
        Ok((data, recon, vectors, uses))
    }

    /// Codes `frame` as its two field pictures after what `writer` holds (M12.38, [`Encoder::set_field_pictures`]):
    /// the field `top_first` says first, then the other. The reconstruction is the two fields woven together.
    fn encode_field_pair(
        &self,
        mut writer: BitWriter,
        frame: &Frame420,
        picture_type: PictureType,
        (temporal_reference, code): (u32, u8),
        top_first: bool,
    ) -> Result<(Vec<u8>, Frame420, Vec<(i32, i32)>, FieldUse), EncodeError> {
        let missing = || EncodeError("missing reference frame".to_string());
        let fields_of = |picture: &Frame420| [motion::field(picture, 0), motion::field(picture, 1)];
        let current = fields_of(frame);
        let (past, future) = match picture_type {
            PictureType::I => (None, None),
            PictureType::P => (
                Some(fields_of(self.reference.as_ref().ok_or_else(missing)?)),
                None,
            ),
            PictureType::B => (
                Some(fields_of(
                    self.older_reference.as_ref().ok_or_else(missing)?,
                )),
                Some(fields_of(self.reference.as_ref().ok_or_else(missing)?)),
            ),
        };
        let first = usize::from(!top_first);
        let second = 1 - first;
        let (search, half_pel) = self.search();
        let qs = u16::from(code).saturating_mul(2);
        let mut done: Option<Frame420> = None;
        for (index, parity) in [first, second].into_iter().enumerate() {
            let Some(cur) = current.get(parity) else {
                continue;
            };
            // An I frame's second field is predicted from its first.
            let field_type = if picture_type == PictureType::I && index == 1 {
                PictureType::P
            } else {
                picture_type
            };
            let (f_code, search) = self.reach(field_type, search);
            write_picture_header(&mut writer, field_type, temporal_reference);
            write_field_picture_coding_extension(&mut writer, field_type, f_code, parity);
            // The reference fields by parity: the most recently decoded reference field of each (13818-2 7.6.2.1),
            // which for a second field is its own frame's first field on that side.
            let mut forward: [Option<&Frame420>; 2] = [None, None];
            let mut backward: [Option<&Frame420>; 2] = [None, None];
            if let Some(past) = &past {
                forward = [past.first(), past.get(1)];
            }
            if let Some(future) = &future {
                backward = [future.first(), future.get(1)];
            }
            if index == 1 && picture_type != PictureType::B {
                if picture_type == PictureType::I {
                    forward = [None, None];
                }
                if let Some(slot) = forward.get_mut(first) {
                    *slot = done.as_ref();
                }
            }
            let coding = Coding {
                code,
                qs,
                f_code,
                search,
                half_pel,
                field: false,
                between: (0, 0),
            };
            let mut recon = empty_frame(cur.width, cur.height);
            let rows = usize::from(cur.height) / 16;
            for row in 0..rows {
                encode_field_row(
                    &mut writer,
                    cur,
                    (field_type, forward, backward),
                    &mut recon,
                    (row, parity),
                    coding,
                );
            }
            if index == 0 {
                done = Some(recon);
            } else if let Some(first_recon) = done.take() {
                let woven = if first == 0 {
                    weave(&first_recon, &recon)
                } else {
                    weave(&recon, &first_recon)
                };
                return Ok((writer.finish(), woven, Vec::new(), FieldUse::default()));
            }
        }
        Err(EncodeError("a field picture was not coded".to_string()))
    }

    /// The sequence header and its extensions, as every GOP starts.
    fn write_sequence_headers(&self, writer: &mut BitWriter) {
        let hd = self.config.format != Format::Sd;
        let frame_rate_code = match (self.progressive, self.config.standard) {
            (Some(rate), _) => rate.code(),
            (None, Standard::Ntsc) => 4,
            (None, Standard::Pal) => 3,
        };
        write_sequence_header(
            writer,
            self.params,
            frame_rate_code,
            bit_rate_value(self.config.max_bitrate),
            if self.config.wide || hd { 3 } else { 2 },
        );
        write_sequence_extension(
            writer,
            self.params.profile_level,
            self.progressive.is_some(),
        );
        if hd {
            write_sequence_display_extension(writer, self.params.display);
        }
    }

    /// Encodes `frame` as a sequence of one I picture at quantiser_scale_code `code` (1 to 31; coarser when the
    /// picture would not fit the VBV buffer), ended by a sequence end code: a menu's still. The encoder's rate
    /// control is not used and does not change.
    ///
    /// # Errors
    ///
    /// When the frame has the wrong size, or does not fit the buffer at the coarsest quantiser.
    pub fn encode_still(&self, frame: &Frame420, code: u8) -> Result<Vec<u8>, EncodeError> {
        validate_frame(frame, self.params.coded)?;
        let mut code = code.clamp(1, 31);
        loop {
            let (mut data, _, _, _) = self.encode_picture(frame, PictureType::I, true, 0, code)?;
            let bits = u64::try_from(data.len())
                .unwrap_or(u64::MAX)
                .saturating_mul(8);
            if bits <= self.params.vbv_bits {
                data.extend_from_slice(&[0, 0, 1, 0xB7]);
                return Ok(data);
            }
            if code >= 31 {
                return Err(EncodeError(format!(
                    "the still takes {bits} bits at quantiser 31, more than the buffer"
                )));
            }
            code = code.saturating_add(1);
        }
    }
}

/// The fields of a picture and of its references, each as a picture of its own ([`motion::field`]; 0 the top): the
/// past reference's, and a B picture's future reference's.
#[derive(Debug)]
struct FieldSet {
    current: [Frame420; 2],
    past: [Frame420; 2],
    future: Option<[Frame420; 2]>,
}

/// The reference pictures a picture is predicted from.
#[derive(Debug, Clone, Copy)]
enum Refs<'a> {
    /// None: an I picture.
    None,
    /// The past one: a P picture; with the fields of both when its macroblocks may use field prediction (M12.36).
    Forward(&'a Frame420, Option<&'a FieldSet>),
    /// The past and the future one: a B picture; with the fields when it may use field prediction (M12.36).
    Both(&'a Frame420, &'a Frame420, Option<&'a FieldSet>),
}

/// How a picture's macroblocks are coded.
#[derive(Debug, Clone, Copy)]
struct Coding {
    /// quantiser_scale_code.
    code: u8,
    /// The quantiser scale (twice the code: the linear scale).
    qs: u16,
    /// f_code of the forward vectors.
    f_code: u8,
    /// Motion search range, pixels.
    search: i32,
    /// Whether vectors are refined to half a pixel.
    half_pel: bool,
    /// Whether macroblocks choose frame or field DCT (an interlaced picture: frame_pred_frame_dct 0).
    field: bool,
    /// A B picture's place between its references: (pictures after the past one, pictures between the two). The
    /// future reference's vectors (`previous`), scaled by it, start its searches.
    between: (i32, i32),
}

/// Encodes the picture's slices (one per macroblock row) into `out` and `recon`, the rows in bands on several
/// threads. Every slice is independent (its predictors start afresh; it reads the picture and the reference and
/// writes only its own rows of the reconstruction), and a slice starts byte-aligned with a start code, so the
/// bands' bytes joined in order are the same as encoding the rows one after another. `previous`: the last picture's
/// vector of each macroblock (M6.394). Returns this picture's.
fn encode_slices(
    frame: &Frame420,
    (reference, previous): (Refs<'_>, &[(i32, i32)]),
    recon: &mut Frame420,
    (coding, workers): (Coding, usize),
    out: &mut Vec<u8>,
) -> Result<(Vec<(i32, i32)>, FieldUse), EncodeError> {
    let mb_rows = usize::from(frame.height) / 16;
    let mb_cols = usize::from(frame.width) / 16;
    let workers = workers.min(mb_rows).max(1);
    let mut vectors = vec![(0, 0); mb_rows * mb_cols];
    let mut uses = FieldUse::default();
    if workers == 1 {
        let mut writer = BitWriter::new();
        for (row, row_vectors) in vectors.chunks_mut(mb_cols.max(1)).enumerate() {
            encode_row(
                (&mut writer, &mut uses),
                frame,
                (reference, previous),
                recon,
                (row, row_vectors),
                coding,
            );
        }
        out.extend_from_slice(&writer.finish());
        return Ok((vectors, uses));
    }
    let per_band = mb_rows.div_ceil(workers);
    let bands: Vec<std::ops::Range<usize>> = (0..workers)
        .map(|band| (band * per_band).min(mb_rows)..((band + 1) * per_band).min(mb_rows))
        .filter(|rows| !rows.is_empty())
        .collect();
    #[allow(clippy::type_complexity)]
    let encoded: Vec<Option<(Vec<u8>, Frame420, Vec<(i32, i32)>, FieldUse)>> =
        std::thread::scope(|scope| {
            let handles: Vec<_> = bands
                .iter()
                .map(|rows| {
                    let rows = rows.clone();
                    scope.spawn(move || {
                        let mut writer = BitWriter::new();
                        let mut band_recon = empty_frame(frame.width, frame.height);
                        let mut band_vectors = vec![(0, 0); rows.len() * mb_cols];
                        let mut band_uses = FieldUse::default();
                        for (row, row_vectors) in rows.zip(band_vectors.chunks_mut(mb_cols.max(1)))
                        {
                            encode_row(
                                (&mut writer, &mut band_uses),
                                frame,
                                (reference, previous),
                                &mut band_recon,
                                (row, row_vectors),
                                coding,
                            );
                        }
                        (writer.finish(), band_recon, band_vectors, band_uses)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().ok())
                .collect()
        });
    let (width, chroma_width) = (usize::from(frame.width), usize::from(frame.width) / 2);
    for (rows, band) in bands.iter().zip(encoded) {
        let (bytes, band_recon, band_vectors, band_uses) =
            band.ok_or_else(|| EncodeError("a slice thread failed".to_string()))?;
        out.extend_from_slice(&bytes);
        uses.coded = uses.coded.saturating_add(band_uses.coded);
        uses.field = uses.field.saturating_add(band_uses.field);
        if let Some(slot) = vectors.get_mut(rows.start * mb_cols..rows.end * mb_cols) {
            slot.copy_from_slice(band_vectors.get(..slot.len()).unwrap_or_default());
        }
        copy_rows(
            &band_recon.y,
            &mut recon.y,
            width,
            rows.start * 16..rows.end * 16,
        );
        copy_rows(
            &band_recon.cb,
            &mut recon.cb,
            chroma_width,
            rows.start * 8..rows.end * 8,
        );
        copy_rows(
            &band_recon.cr,
            &mut recon.cr,
            chroma_width,
            rows.start * 8..rows.end * 8,
        );
    }
    Ok((vectors, uses))
}

/// How many of a picture's coded macroblocks carried the field tools' modes, and how many of them took a field
/// prediction or a field DCT (M12.36).
#[derive(Debug, Clone, Copy, Default)]
struct FieldUse {
    coded: u32,
    field: u32,
}

impl FieldUse {
    /// Notes a coded macroblock of a picture with the field tools: whether it took one of them.
    fn note(&mut self, took: bool) {
        self.coded = self.coded.saturating_add(1);
        self.field = self.field.saturating_add(u32::from(took));
    }

    /// The share of the coded macroblocks that took a field tool; `None` for a picture without them.
    fn share(self) -> Option<f64> {
        (self.coded > 0).then(|| f64::from(self.field) / f64::from(self.coded))
    }
}

/// Most threads one picture's slices use.
const MAX_SLICE_THREADS: usize = 16;

/// How many threads encode a picture of `mb_rows` macroblock rows: the CPU's cores, at most 16.
fn slice_workers(mb_rows: usize) -> usize {
    std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(MAX_SLICE_THREADS)
        .min(mb_rows)
        .max(1)
}

/// Copies lines `lines` of a plane `width` wide.
fn copy_rows(from: &[u8], to: &mut [u8], width: usize, lines: std::ops::Range<usize>) {
    let range = lines.start.saturating_mul(width)..lines.end.saturating_mul(width);
    if let (Some(source), Some(target)) = (from.get(range.clone()), to.get_mut(range)) {
        target.copy_from_slice(source);
    }
}

/// Encodes macroblock row `row` as one slice: intra without a reference, else P (`previous`: the last picture's
/// vectors, M6.394); each macroblock's vector into `vectors`.
fn encode_row(
    (writer, uses): (&mut BitWriter, &mut FieldUse),
    frame: &Frame420,
    (reference, previous): (Refs<'_>, &[(i32, i32)]),
    recon: &mut Frame420,
    (row, vectors): (usize, &mut [(i32, i32)]),
    coding: Coding,
) {
    let mb_cols = usize::from(frame.width) / 16;
    let qs = coding.qs;
    write_slice_header(writer, row, coding.code);
    let mut predictors = Predictors::new();
    let y = row * 16;
    match reference {
        Refs::None => {
            for col in 0..mb_cols {
                encode_intra_macroblock(
                    writer,
                    frame,
                    recon,
                    (col * 16, y),
                    (qs, coding.field),
                    &mut predictors,
                );
            }
        }
        Refs::Both(past, future, fields) => {
            let mut state = BState::default();
            let (after, span) = coding.between;
            for col in 0..mb_cols {
                // The future reference's vector here spans the two references: this picture's share of it forward,
                // the rest of it back.
                let vector = previous.get(row * mb_cols + col).copied().unwrap_or((0, 0));
                let scaled = |share: i32| {
                    if span > 0 {
                        (vector.0 * share / span, vector.1 * share / span)
                    } else {
                        (0, 0)
                    }
                };
                let temporal = (scaled(after), scaled(after - span));
                let chosen = encode_b_macroblock(
                    writer,
                    frame,
                    (past, future, fields, temporal),
                    recon,
                    (col * 16, y),
                    coding,
                    &mut predictors,
                    (&mut state, &mut *uses),
                    // A slice's first and last macroblocks are always coded (13818-2 6.3.16).
                    col > 0 && col + 1 < mb_cols,
                );
                if let Some(slot) = vectors.get_mut(col) {
                    *slot = chosen;
                }
            }
        }
        Refs::Forward(reference, fields) => {
            let mut prev_intra = false;
            let mut pmv = [(0i32, 0i32); 2];
            let mut skipped = 0u32;
            for col in 0..mb_cols {
                let temporal = previous.get(row * mb_cols + col).copied().unwrap_or((0, 0));
                let chosen = encode_p_macroblock(
                    writer,
                    frame,
                    (reference, fields, temporal),
                    recon,
                    col * 16,
                    y,
                    coding,
                    &mut predictors,
                    &mut prev_intra,
                    &mut pmv,
                    (&mut skipped, &mut *uses),
                    // A slice's first and last macroblocks are always coded (13818-2 6.3.16).
                    col > 0 && col + 1 < mb_cols,
                );
                if let Some(slot) = vectors.get_mut(col) {
                    *slot = chosen;
                }
            }
        }
    }
}

fn bit_rate_value(max_bitrate: u32) -> u32 {
    max_bitrate.saturating_add(399) / 400
}

fn validate_frame(frame: &Frame420, coded: (u16, u16)) -> Result<(), EncodeError> {
    let w = usize::from(frame.width);
    let h = usize::from(frame.height);

    if (frame.width, frame.height) != coded {
        return Err(EncodeError(format!(
            "expected {}x{}, got {w}x{h}",
            coded.0, coded.1
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
    params: Params,
    frame_rate_code: u32,
    bit_rate_value: u32,
    aspect_code: u32,
) {
    writer.write_start_code(0xB3);
    writer.write_bits(u32::from(params.display.0), 12);
    writer.write_bits(u32::from(params.display.1), 12);
    writer.write_bits(aspect_code, 4);
    writer.write_bits(frame_rate_code, 4);
    writer.write_bits(bit_rate_value, 18);
    writer.write_bits(1, 1);
    // vbv_buffer_size_value, units of 16,384 bits: 112 (MP@ML) or 597 (MP@HL).
    writer.write_bits(u32::try_from(params.vbv_bits / 16_384).unwrap_or(112), 10);
    writer.write_bits(0, 1);
    writer.write_bits(0, 1);
    writer.write_bits(0, 1);
}

/// The sequence extension (13818-2 6.2.2.3), 48 bits after its start code: a progressive sequence (only
/// progressive frames) or one whose frames may be interlaced.
fn write_sequence_extension(writer: &mut BitWriter, profile_level: u32, progressive: bool) {
    writer.write_start_code(0xB5);
    writer.write_bits(1, 4); // extension_start_code_identifier: sequence extension
    writer.write_bits(profile_level, 8);
    writer.write_bits(u32::from(progressive), 1); // progressive_sequence
    writer.write_bits(1, 2); // chroma_format: 4:2:0
    writer.write_bits(0, 2); // horizontal_size_extension
    writer.write_bits(0, 2); // vertical_size_extension
    writer.write_bits(0, 12); // bit_rate_extension
    writer.write_bits(1, 1); // marker_bit
    writer.write_bits(0, 8); // vbv_buffer_size_extension
    writer.write_bits(0, 1); // low_delay
    writer.write_bits(0, 2); // frame_rate_extension_n
    writer.write_bits(0, 5); // frame_rate_extension_d
}

/// The sequence display extension (13818-2 6.2.2.4) of an HD sequence: BT.709 primaries, transfer and matrix, and
/// the size shown.
fn write_sequence_display_extension(writer: &mut BitWriter, display: (u16, u16)) {
    writer.write_start_code(0xB5);
    writer.write_bits(2, 4); // extension_start_code_identifier: sequence display extension
    writer.write_bits(0, 3); // video_format: component
    writer.write_bits(1, 1); // colour_description
    writer.write_bits(1, 8); // colour_primaries: BT.709
    writer.write_bits(1, 8); // transfer_characteristics: BT.709
    writer.write_bits(1, 8); // matrix_coefficients: BT.709
    writer.write_bits(u32::from(display.0), 14); // display_horizontal_size
    writer.write_bits(1, 1); // marker_bit
    writer.write_bits(u32::from(display.1), 14); // display_vertical_size
    writer.write_bits(0, 3); // to the byte
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
        PictureType::B => {
            writer.write_bits(3, 3);
            writer.write_bits(0xFFFF, 16);
            // full_pel_forward_vector and forward_f_code, then the backward ones: fixed in MPEG-2 (the f codes are in
            // the picture coding extension).
            writer.write_bits(0, 1);
            writer.write_bits(7, 3);
            writer.write_bits(0, 1);
            writer.write_bits(7, 3);
        }
    }
}

/// The picture coding extension of a frame picture; `(top_field_first, repeat_first_field)` as it is shown.
fn write_picture_coding_extension(
    writer: &mut BitWriter,
    picture_type: PictureType,
    f_code: u8,
    (top_field_first, repeat_first_field): (bool, bool),
    (interlaced, field_tools): (bool, bool),
) {
    let f = u32::from(f_code);
    // f_code[0][*] for the forward vectors, f_code[1][*] for the backward ones; 15 where there are none.
    let (forward, backward) = match picture_type {
        PictureType::I => (15, 15),
        PictureType::P => (f, 15),
        PictureType::B => (f, f),
    };

    writer.write_start_code(0xB5);
    writer.write_bits(8, 4);
    writer.write_bits(forward, 4);
    writer.write_bits(forward, 4);
    writer.write_bits(backward, 4);
    writer.write_bits(backward, 4);
    writer.write_bits(0, 2); // intra_dc_precision: 8 bits
    writer.write_bits(3, 2); // picture_structure: frame
    writer.write_bits(u32::from(top_field_first), 1);
    // frame_pred_frame_dct: 0 lets each macroblock choose frame or field DCT (with frame prediction); an
    // interlaced picture coded with frame tools only keeps 1 (M12.26).
    writer.write_bits(u32::from(!field_tools), 1);
    writer.write_bits(0, 1); // concealment_motion_vectors
    writer.write_bits(0, 1); // q_scale_type: linear
    writer.write_bits(1, 1); // intra_vlc_format: table one (B.15) for intra AC coefficients
    writer.write_bits(0, 1); // alternate_scan
    writer.write_bits(u32::from(repeat_first_field), 1);
    // chroma_420_type: as progressive_frame (13818-2 6.3.10; M6.394).
    writer.write_bits(u32::from(!interlaced), 1);
    writer.write_bits(u32::from(!interlaced), 1); // progressive_frame
    writer.write_bits(0, 1); // composite_display_flag
}

/// The picture coding extension of a field picture (M12.38) of `parity` (0: the top field, picture_structure 1;
/// 1: the bottom one, 2): top_field_first and frame_pred_frame_dct 0, as a field picture has them (13818-2 6.3.10),
/// not progressive.
fn write_field_picture_coding_extension(
    writer: &mut BitWriter,
    picture_type: PictureType,
    f_code: u8,
    parity: usize,
) {
    let f = u32::from(f_code);
    let (forward, backward) = match picture_type {
        PictureType::I => (15, 15),
        PictureType::P => (f, 15),
        PictureType::B => (f, f),
    };
    writer.write_start_code(0xB5);
    writer.write_bits(8, 4);
    writer.write_bits(forward, 4);
    writer.write_bits(forward, 4);
    writer.write_bits(backward, 4);
    writer.write_bits(backward, 4);
    writer.write_bits(0, 2); // intra_dc_precision: 8 bits
    writer.write_bits(if parity == 0 { 1 } else { 2 }, 2); // picture_structure
    writer.write_bits(0, 1); // top_field_first
    writer.write_bits(0, 1); // frame_pred_frame_dct
    writer.write_bits(0, 1); // concealment_motion_vectors
    writer.write_bits(0, 1); // q_scale_type: linear
    writer.write_bits(1, 1); // intra_vlc_format
    writer.write_bits(0, 1); // alternate_scan
    writer.write_bits(0, 1); // repeat_first_field
    writer.write_bits(0, 1); // chroma_420_type
    writer.write_bits(0, 1); // progressive_frame
    writer.write_bits(0, 1); // composite_display_flag
}

/// A frame of the two fields `top` and `bottom` (each half its height): their lines one after the other.
fn weave(top: &Frame420, bottom: &Frame420) -> Frame420 {
    let mut frame = empty_frame(top.width, top.height.saturating_mul(2));
    let planes = [
        (&mut frame.y, &top.y, &bottom.y, usize::from(top.width)),
        (
            &mut frame.cb,
            &top.cb,
            &bottom.cb,
            usize::from(top.width) / 2,
        ),
        (
            &mut frame.cr,
            &top.cr,
            &bottom.cr,
            usize::from(top.width) / 2,
        ),
    ];
    for (woven, top, bottom, width) in planes {
        if width == 0 {
            continue;
        }
        for (pair, (top_line, bottom_line)) in woven
            .chunks_mut(width * 2)
            .zip(top.chunks(width).zip(bottom.chunks(width)))
        {
            let (upper, lower) = pair.split_at_mut(width.min(pair.len()));
            upper.copy_from_slice(top_line.get(..upper.len()).unwrap_or_default());
            lower.copy_from_slice(bottom_line.get(..lower.len()).unwrap_or_default());
        }
    }
    frame
}

/// What a field picture's slice carries from one macroblock to the next (M12.38).
#[derive(Debug, Clone, Copy, Default)]
struct FieldSlice {
    prev_intra: bool,
    /// The forward and backward motion vector predictors (half pixels, the vertical in field lines).
    pmv: [(i32, i32); 2],
    skipped: u32,
}

/// Encodes macroblock row `row` of a field picture of `parity` as one slice: intra in an I field, else from the
/// reference fields by parity, `forward` (and in a B field `backward`).
fn encode_field_row(
    writer: &mut BitWriter,
    cur: &Frame420,
    (picture_type, forward, backward): (
        PictureType,
        [Option<&Frame420>; 2],
        [Option<&Frame420>; 2],
    ),
    recon: &mut Frame420,
    (row, parity): (usize, usize),
    coding: Coding,
) {
    let mb_cols = usize::from(cur.width) / 16;
    write_slice_header(writer, row, coding.code);
    let mut predictors = Predictors::new();
    let mut slice = FieldSlice::default();
    for col in 0..mb_cols {
        let place = (col * 16, row * 16);
        // A slice's first and last macroblocks are always coded (13818-2 6.3.16).
        let can_skip = col > 0 && col + 1 < mb_cols;
        match picture_type {
            PictureType::I => {
                encode_intra_macroblock(
                    writer,
                    cur,
                    recon,
                    place,
                    (coding.qs, false),
                    &mut predictors,
                );
            }
            PictureType::P => encode_field_p_macroblock(
                writer,
                (cur, forward, parity),
                recon,
                place,
                coding,
                (&mut predictors, &mut slice, can_skip),
            ),
            PictureType::B => encode_field_b_macroblock(
                writer,
                (cur, forward, backward),
                recon,
                place,
                coding,
                (&mut predictors, &mut slice),
            ),
        }
    }
}

/// The closest of the reference fields `refs` (by parity) for the macroblock at `(x, y)` of the field `cur`: its
/// parity (motion_vertical_field_select) and vector (half pixels) with its SAD; `None` without a reference.
fn search_field_refs(
    cur: &Frame420,
    refs: &[Option<&Frame420>; 2],
    (x, y): (usize, usize),
    coding: Coding,
    pmv: (i32, i32),
) -> Option<(usize, (i32, i32), u32)> {
    refs.iter()
        .enumerate()
        .filter_map(|(select, reference)| reference.map(|reference| (select, reference)))
        .map(|(select, reference)| {
            let found = motion::search_fast(
                cur,
                reference,
                (x, y),
                coding.search,
                &[(pmv.0 / 2, pmv.1 / 2)],
            );
            let (vx, vy, sad) = if coding.half_pel {
                motion::refine_half(cur, reference, (x, y), found, coding.search)
            } else {
                (
                    found.0.saturating_mul(2),
                    found.1.saturating_mul(2),
                    found.2,
                )
            };
            (select, (vx, vy), sad)
        })
        .min_by_key(|(_, _, sad)| *sad)
}

/// An intra macroblock of a P or B field picture: no dct_type (a field picture has none); the vector predictors
/// reset.
fn encode_field_intra(
    writer: &mut BitWriter,
    (cur, recon): (&Frame420, &mut Frame420),
    (x, y): (usize, usize),
    (qs, bidirectional): (u16, bool),
    (predictors, slice): (&mut Predictors, &mut FieldSlice),
) {
    vlc::write_macroblock_address_increment_after(writer, slice.skipped);
    slice.skipped = 0;
    if bidirectional {
        vlc::write_b_macroblock_type(writer, vlc::BType::Intra);
    } else {
        vlc::write_p_macroblock_type(writer, vlc::PType::Intra);
    }
    if !slice.prev_intra {
        *predictors = Predictors::new();
    }
    encode_intra_blocks(writer, cur, recon, x, y, qs, false, predictors);
    slice.prev_intra = true;
    slice.pmv = [(0, 0); 2];
}

/// One field vector: motion_vertical_field_select, then the vector from its predictor, which becomes it
/// (13818-2 6.2.5.2, 7.6.3.1: a field picture's vertical vectors are in field lines, not scaled).
fn write_field_picture_vector(
    writer: &mut BitWriter,
    (select, vector): (usize, (i32, i32)),
    pmv: &mut (i32, i32),
    f_code: u8,
) {
    writer.write_bits(u32::from(select == 1), 1);
    vlc::write_motion_delta(
        writer,
        vlc::wrap_motion_delta(vector.0 - pmv.0, f_code),
        f_code,
    );
    vlc::write_motion_delta(
        writer,
        vlc::wrap_motion_delta(vector.1 - pmv.1, f_code),
        f_code,
    );
    *pmv = vector;
}

/// A P field picture's macroblock (M12.38): predicted from the closer of its reference fields (`refs`, by parity),
/// intra when that is clearly cheaper, skipped when it is the field of its own parity with no motion and nothing to
/// code (what a skipped macroblock of a P field picture repeats, 13818-2 7.6.6.2).
fn encode_field_p_macroblock(
    writer: &mut BitWriter,
    (cur, refs, parity): (&Frame420, [Option<&Frame420>; 2], usize),
    recon: &mut Frame420,
    (x, y): (usize, usize),
    coding: Coding,
    (predictors, slice, can_skip): (&mut Predictors, &mut FieldSlice, bool),
) {
    let [pmv, _] = slice.pmv;
    let best = search_field_refs(cur, &refs, (x, y), coding, pmv);
    let intra = motion::intra_cost(cur, x, y).saturating_add(512);
    let chosen = best
        .filter(|(_, _, sad)| intra >= *sad)
        .and_then(|(select, vector, _)| {
            Some((select, vector, refs.get(select).copied().flatten()?))
        });
    let Some((select, vector, reference)) = chosen else {
        encode_field_intra(
            writer,
            (cur, recon),
            (x, y),
            (coding.qs, false),
            (predictors, slice),
        );
        return;
    };
    let prediction = motion::predict(reference, x, y, vector.0, vector.1);
    let levels = quantise_inter(cur, (x, y), &prediction, coding.qs, false);
    if can_skip && levels.cbp == 0 && vector == (0, 0) && select == parity {
        slice.skipped = slice.skipped.saturating_add(1);
        slice.pmv = [(0, 0); 2];
    } else {
        vlc::write_macroblock_address_increment_after(writer, slice.skipped);
        slice.skipped = 0;
        vlc::write_p_macroblock_type(
            writer,
            if levels.cbp != 0 {
                vlc::PType::McCoded
            } else {
                vlc::PType::McNotCoded
            },
        );
        writer.write_bits(1, 2); // field_motion_type: field-based (13818-2 table 6-18)
        if let Some(predictor) = slice.pmv.first_mut() {
            write_field_picture_vector(writer, (select, vector), predictor, coding.f_code);
        }
        if levels.cbp != 0 {
            vlc::write_coded_block_pattern(writer, reverse_cbp(levels.cbp));
        }
    }
    write_inter_blocks(writer, recon, (x, y), &prediction, &levels, coding.qs);
    slice.prev_intra = false;
}

/// A B field picture's macroblock (M12.38): the closest of the forward, backward and interpolated predictions,
/// each from the closer field of its reference frame; intra when that is clearly cheaper. Never skipped (a skipped
/// one would repeat the last macroblock's fields too).
fn encode_field_b_macroblock(
    writer: &mut BitWriter,
    (cur, forward, backward): (&Frame420, [Option<&Frame420>; 2], [Option<&Frame420>; 2]),
    recon: &mut Frame420,
    (x, y): (usize, usize),
    coding: Coding,
    (predictors, slice): (&mut Predictors, &mut FieldSlice),
) {
    use vlc::BPrediction;
    let [pmv_forward, pmv_backward] = slice.pmv;
    let found = (
        search_field_refs(cur, &forward, (x, y), coding, pmv_forward),
        search_field_refs(cur, &backward, (x, y), coding, pmv_backward),
    );
    let (Some((select_f, vector_f, sad_f)), Some((select_b, vector_b, sad_b))) = found else {
        encode_field_intra(
            writer,
            (cur, recon),
            (x, y),
            (coding.qs, true),
            (predictors, slice),
        );
        return;
    };
    let (Some(past), Some(future)) = (
        forward.get(select_f).copied().flatten(),
        backward.get(select_b).copied().flatten(),
    ) else {
        encode_field_intra(
            writer,
            (cur, recon),
            (x, y),
            (coding.qs, true),
            (predictors, slice),
        );
        return;
    };
    let prediction_f = motion::predict(past, x, y, vector_f.0, vector_f.1);
    let prediction_b = motion::predict(future, x, y, vector_b.0, vector_b.1);
    let interpolated = average_prediction(&prediction_f, &prediction_b);
    let sad_i = motion::sad_prediction(cur, x, y, &interpolated.0).saturating_add(32);
    let (mode, sad) = [
        (BPrediction::Interpolated, sad_i),
        (BPrediction::Forward, sad_f),
        (BPrediction::Backward, sad_b),
    ]
    .into_iter()
    .min_by_key(|(_, sad)| *sad)
    .unwrap_or((BPrediction::Forward, sad_f));
    if motion::intra_cost(cur, x, y).saturating_add(512) < sad {
        encode_field_intra(
            writer,
            (cur, recon),
            (x, y),
            (coding.qs, true),
            (predictors, slice),
        );
        return;
    }
    let prediction = match mode {
        BPrediction::Forward => prediction_f,
        BPrediction::Backward => prediction_b,
        BPrediction::Interpolated => interpolated,
    };
    let levels = quantise_inter(cur, (x, y), &prediction, coding.qs, false);
    vlc::write_macroblock_address_increment_after(writer, slice.skipped);
    slice.skipped = 0;
    vlc::write_b_macroblock_type(writer, vlc::BType::Mc(mode, levels.cbp != 0));
    writer.write_bits(1, 2); // field_motion_type: field-based
    let [predictor_f, predictor_b] = &mut slice.pmv;
    if mode != BPrediction::Backward {
        write_field_picture_vector(writer, (select_f, vector_f), predictor_f, coding.f_code);
    }
    if mode != BPrediction::Forward {
        write_field_picture_vector(writer, (select_b, vector_b), predictor_b, coding.f_code);
    }
    if levels.cbp != 0 {
        vlc::write_coded_block_pattern(writer, reverse_cbp(levels.cbp));
    }
    write_inter_blocks(writer, recon, (x, y), &prediction, &levels, coding.qs);
    slice.prev_intra = false;
}

fn write_slice_header(writer: &mut BitWriter, row: usize, code: u8) {
    let slice_code = u8::try_from(row + 1).unwrap_or(1);
    writer.write_start_code(slice_code);
    writer.write_bits(u32::from(code), 5);
    writer.write_bits(0, 1);
}

fn encode_intra_macroblock(
    writer: &mut BitWriter,
    frame: &Frame420,
    recon: &mut Frame420,
    (x, y): (usize, usize),
    (qs, field): (u16, bool),
    predictors: &mut Predictors,
) {
    writer.write_bits(1, 1);
    writer.write_bits(1, 1);
    let field_dct = field && intra_field_dct(frame, x, y, qs);
    if field {
        writer.write_bits(u32::from(field_dct), 1); // dct_type
    }
    encode_intra_blocks(writer, frame, recon, x, y, qs, field_dct, predictors);
}

/// Where the four luma blocks of the macroblock at `(x, y)` of a plane `w` wide are (the plane's width as read,
/// x, y), and where they are in its 16 x 16 prediction (stride, x, y): the frame's quarters, or with field DCT
/// (13818-2 6.1.3, figure 6-13) the top field's two halves then the bottom field's, read from the plane seen at
/// twice its width, where a row is two of the frame's (M6.371).
fn luma_layout(
    field_dct: bool,
    w: usize,
    (x, y): (usize, usize),
) -> [((usize, usize, usize), (usize, usize, usize)); 4] {
    if field_dct {
        let (wide, row) = (w * 2, y / 2);
        [
            ((wide, x, row), (32, 0, 0)),
            ((wide, x + 8, row), (32, 8, 0)),
            ((wide, x + w, row), (32, 16, 0)),
            ((wide, x + w + 8, row), (32, 24, 0)),
        ]
    } else {
        [
            ((w, x, y), (16, 0, 0)),
            ((w, x + 8, y), (16, 8, 0)),
            ((w, x, y + 8), (16, 0, 8)),
            ((w, x + 8, y + 8), (16, 8, 8)),
        ]
    }
}

/// What a macroblock's four quantised luma blocks would cost: a rough count of the bits of their coefficients
/// (each coefficient that is not 0, and the size of its level).
fn levels_cost(blocks: &[[i16; 64]; 4]) -> u32 {
    blocks
        .iter()
        .flatten()
        .filter(|level| **level != 0)
        .map(|level| 4 + (16 - level.unsigned_abs().leading_zeros()))
        .sum()
}

/// Whether the intra macroblock at `(x, y)` costs less transformed as two fields than as a frame (M6.371): both
/// are quantised at `qs` and the cheaper kept.
/// The least share of a P picture's coded macroblocks that take a field tool ([`FieldUse`]) for the next P picture to
/// keep the field tools (M12.36). Measured on 59.94p material woven into interlaced frames: a pan and a moving test
/// pattern took them in 18–50 % of the macroblocks and gained 0.3–0.7 dB over frames; a slow zoom with fine detail
/// took them in 9–22 % and lost to frames with them.
const FIELD_TOOLS_SHARE_P: f64 = 0.18;

/// The same for B pictures, which take the field tools less (their two references already predict well): 9–39 %
/// where they paid, 7–14 % where they did not.
const FIELD_TOOLS_SHARE_B: f64 = 0.13;

fn intra_field_dct(frame: &Frame420, x: usize, y: usize, qs: u16) -> bool {
    let w = usize::from(frame.width);
    let levels = |field: bool| {
        luma_layout(field, w, (x, y)).map(|((pw, bx, by), _)| {
            quant::quantise_intra(&dct::forward_dct8x8(&block8x8(&frame.y, pw, bx, by)), qs)
        })
    };
    levels_cost(&levels(true)) < levels_cost(&levels(false))
}

#[allow(clippy::too_many_arguments)] // TODO: group into a context struct (tech debt)
fn encode_intra_blocks(
    writer: &mut BitWriter,
    frame: &Frame420,
    recon: &mut Frame420,
    x: usize,
    y: usize,
    qs: u16,
    field_dct: bool,
    predictors: &mut Predictors,
) {
    let w = usize::from(frame.width);
    let cw = w / 2;

    for ((plane_width, bx, by), _) in luma_layout(field_dct, w, (x, y)) {
        encode_intra_block(
            writer,
            &frame.y,
            plane_width,
            bx,
            by,
            qs,
            true,
            &mut predictors.y,
            &mut recon.y,
            plane_width,
            bx,
            by,
        );
    }

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

/// A macroblock's field prediction in a frame picture (M12.36, 13818-2 7.6.3): for its top and bottom field (0, 1)
/// the reference field it is predicted from (motion_vertical_field_select, 0 the top) and the vector in half pixels,
/// its vertical part in field lines; and the two fields' SAD together.
#[derive(Clone, Copy, Debug)]
struct FieldMotion {
    select: [usize; 2],
    vectors: [(i32, i32); 2],
    sad: u32,
}

/// The best field prediction of the macroblock at (`x`, `y`): each of its fields searched in both fields of the
/// reference, starting from the frame vector `frame` (half pixels) as field lines between the two fields.
fn search_fields(
    (current_fields, reference_fields): (&[Frame420; 2], &[Frame420; 2]),
    (x, y): (usize, usize),
    coding: Coding,
    frame: (i32, i32),
) -> FieldMotion {
    let mut motion = FieldMotion {
        select: [0; 2],
        vectors: [(0, 0); 2],
        sad: 0,
    };
    let (whole_x, whole_y) = (frame.0 >> 1, frame.1 >> 1);
    for (parity, current) in current_fields.iter().enumerate() {
        let mut best: Option<(usize, (i32, i32, u32))> = None;
        for (select, reference) in reference_fields.iter().enumerate() {
            // A frame line `whole_y` away, from a line of this field to one of that field.
            let lines =
                i32::try_from(parity).unwrap_or(0) + whole_y - i32::try_from(select).unwrap_or(0);
            let start = (whole_x, lines.div_euclid(2));
            let found = motion::search_fast_rows(
                current,
                reference,
                (x, y / 2, 8),
                coding.search,
                &[start],
            );
            let found = if coding.half_pel {
                motion::refine_half_rows(current, reference, (x, y / 2, 8), found, coding.search)
            } else {
                (
                    found.0.saturating_mul(2),
                    found.1.saturating_mul(2),
                    found.2,
                )
            };
            if best.is_none_or(|(_, kept)| found.2 < kept.2) {
                best = Some((select, found));
            }
        }
        if let (Some((select, (vx, vy, sad))), Some(slot), Some(vector)) = (
            best,
            motion.select.get_mut(parity),
            motion.vectors.get_mut(parity),
        ) {
            *slot = select;
            *vector = (vx, vy);
            motion.sad = motion.sad.saturating_add(sad);
        }
    }
    motion
}

/// The macroblock's prediction from its two field predictions, each field's lines in their places.
fn predict_fields(
    reference_fields: &[Frame420; 2],
    (x, y): (usize, usize),
    motion: &FieldMotion,
) -> ([u8; 256], [u8; 64], [u8; 64]) {
    let (mut luma, mut cb, mut cr) = ([0u8; 256], [0u8; 64], [0u8; 64]);
    for parity in 0..2 {
        let select = motion.select.get(parity).copied().unwrap_or(0);
        let vector = motion.vectors.get(parity).copied().unwrap_or((0, 0));
        let Some(reference) = reference_fields.get(select) else {
            continue;
        };
        let (field_y, field_cb, field_cr) = motion::predict_field(reference, (x, y), vector);
        for line in 0..8 {
            let (from, to) = (line * 16, (2 * line + parity) * 16);
            if let (Some(source), Some(target)) =
                (field_y.get(from..from + 16), luma.get_mut(to..to + 16))
            {
                target.copy_from_slice(source);
            }
        }
        for line in 0..4 {
            let (from, to) = (line * 8, (2 * line + parity) * 8);
            for (plane, field) in [(&mut cb, &field_cb), (&mut cr, &field_cr)] {
                if let (Some(source), Some(target)) =
                    (field.get(from..from + 8), plane.get_mut(to..to + 8))
                {
                    target.copy_from_slice(source);
                }
            }
        }
    }
    (luma, cb, cr)
}

#[allow(clippy::too_many_arguments)] // TODO: group into a context struct (tech debt)
fn encode_p_macroblock(
    writer: &mut BitWriter,
    cur: &Frame420,
    (reference, fields, temporal): (&Frame420, Option<&FieldSet>, (i32, i32)),
    recon: &mut Frame420,
    x: usize,
    y: usize,
    coding: Coding,
    predictors: &mut Predictors,
    prev_intra: &mut bool,
    pmv: &mut [(i32, i32); 2],
    (skipped, uses): (&mut u32, &mut FieldUse),
    can_skip: bool,
) -> (i32, i32) {
    let (qs, f_code) = (coding.qs, coding.f_code);
    let [first_pmv, _] = *pmv;
    // The search starts from the best of no motion, the vector before this one, and (M6.394) the last picture's
    // vector here: started from no motion alone, interlaced pictures fell into vectors that take one field for the
    // other.
    let (dx, dy, sad) = motion::search_fast(
        cur,
        reference,
        (x, y),
        coding.search,
        &[
            (first_pmv.0 / 2, first_pmv.1 / 2),
            (temporal.0 / 2, temporal.1 / 2),
        ],
    );
    // To half a pixel around the best whole-pixel vector (a low quality keeps whole pixels).
    let (mv_x, mv_y, sad) = if coding.half_pel {
        motion::refine_half(cur, reference, (x, y), (dx, dy, sad), coding.search)
    } else {
        (dx.saturating_mul(2), dy.saturating_mul(2), sad)
    };
    // M12.36: each field predicted on its own when that is clearly closer (it takes two vectors and their fields).
    let field_motion = fields
        .map(|set| {
            let motion = search_fields((&set.current, &set.past), (x, y), coding, (mv_x, mv_y));
            (set, motion)
        })
        .filter(|(_, motion)| {
            motion
                .sad
                .saturating_add(u32::from(qs).saturating_mul(FIELD_MOTION_PENALTY))
                < sad
        });
    let sad = field_motion.map_or(sad, |(_, motion)| motion.sad);
    let intra = motion::intra_cost(cur, x, y);

    if intra.saturating_add(512) < sad {
        vlc::write_macroblock_address_increment_after(writer, *skipped);
        *skipped = 0;
        vlc::write_p_macroblock_type(writer, vlc::PType::Intra);
        let field_dct = coding.field && intra_field_dct(cur, x, y, qs);
        if coding.field {
            writer.write_bits(u32::from(field_dct), 1); // dct_type
        }

        if !*prev_intra {
            *predictors = Predictors::new();
        }

        encode_intra_blocks(writer, cur, recon, x, y, qs, field_dct, predictors);

        *prev_intra = true;
        *pmv = [(0, 0); 2];
        return (0, 0);
    }

    let (pred_y, pred_cb, pred_cr) = match &field_motion {
        Some((set, motion)) => predict_fields(&set.past, (x, y), motion),
        None => motion::predict(reference, x, y, mv_x, mv_y),
    };

    let w = usize::from(cur.width);
    let cw = w / 2;

    // An interlaced picture's macroblock transforms its residual as two fields when they are closer (M6.371).
    let quantised = |field: bool| {
        luma_layout(field, w, (x, y)).map(|((pw, bx, by), (ps, px, py))| {
            quantise_residual_block(&cur.y, pw, bx, by, &pred_y, ps, px, py, qs)
        })
    };
    let frame_levels = quantised(false);
    let (field_dct, luma_levels) = if coding.field {
        let field_levels = quantised(true);
        if levels_cost(&field_levels) < levels_cost(&frame_levels) {
            (true, field_levels)
        } else {
            (false, frame_levels)
        }
    } else {
        (false, frame_levels)
    };
    let layout = luma_layout(field_dct, w, (x, y));
    let [levels0, levels1, levels2, levels3] = luma_levels;
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

    // A skipped macroblock (P picture): no motion and no coefficients, so the decoder copies the reference at the same
    // place, resets the motion vector predictors to zero (13818-2 7.6.6.2) and the DC predictors (7.2.1); the
    // reconstruction below is exactly that copy.
    if can_skip && cbp == 0 && mv_x == 0 && mv_y == 0 && field_motion.is_none() {
        *skipped = skipped.saturating_add(1);
        *pmv = [(0, 0); 2];
    } else {
        vlc::write_macroblock_address_increment_after(writer, *skipped);
        *skipped = 0;

        // Syntax order (13818-2 §6.2.5): macroblock_modes, motion_vectors, coded_block_pattern, blocks.
        if cbp != 0 {
            vlc::write_p_macroblock_type(writer, vlc::PType::McCoded);
        } else {
            vlc::write_p_macroblock_type(writer, vlc::PType::McNotCoded);
        }
        if coding.field {
            // frame_motion_type: field-based (01) or frame-based (10) prediction (13818-2 table 6-17); then dct_type
            // when coded.
            writer.write_bits(if field_motion.is_some() { 1 } else { 2 }, 2);
            if cbp != 0 {
                writer.write_bits(u32::from(field_dct), 1);
            }
            uses.note(field_motion.is_some() || (cbp != 0 && field_dct));
        }

        match &field_motion {
            // Two field vectors, each after its field select bit; a vertical component is predicted from half its
            // predictor and kept doubled (13818-2 7.6.3.1).
            Some((_, motion)) => write_field_vectors(writer, motion, pmv, f_code),
            // One frame vector; both predictors become it.
            None => {
                let delta_x = vlc::wrap_motion_delta(mv_x - first_pmv.0, f_code);
                let delta_y = vlc::wrap_motion_delta(mv_y - first_pmv.1, f_code);
                vlc::write_motion_delta(writer, delta_x, f_code);
                vlc::write_motion_delta(writer, delta_y, f_code);
                *pmv = [(mv_x, mv_y); 2];
            }
        }

        if cbp != 0 {
            // Internally bit 0 is block 0 (Y0); in the bitstream block 0 is the MSB of the 6-bit pattern.
            vlc::write_coded_block_pattern(writer, reverse_cbp(cbp));
        }
    }

    for (index, (((pw, bx, by), (ps, px, py)), levels)) in layout
        .iter()
        .zip([&levels0, &levels1, &levels2, &levels3])
        .enumerate()
    {
        let coded = cbp & (1 << index) != 0;
        if coded {
            write_non_intra_block(writer, levels);
        }
        reconstruct_inter_block(
            &mut recon.y,
            *pw,
            *bx,
            *by,
            &pred_y,
            *ps,
            *px,
            *py,
            coded.then_some(levels),
            qs,
        );
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
    // The next picture's searches start from a frame vector: a field-predicted macroblock's top field's.
    field_motion.map_or((mv_x, mv_y), |(_, motion)| {
        let (vx, vy) = motion.vectors[0];
        (vx, vy.saturating_mul(2))
    })
}

/// How much closer (in SAD per unit of the quantiser scale) a field prediction must be than the frame one to be taken:
/// it codes a second vector and two field select bits.
const FIELD_MOTION_PENALTY: u32 = 4;

/// What a B picture's slice carries from one macroblock to the next.
#[derive(Debug, Clone, Copy, Default)]
struct BState {
    /// Whether the last macroblock was intra (the next one may not be skipped, and resets the DC predictors).
    prev_intra: bool,
    /// The forward and backward motion vector predictors (half pixels): PMV[0] and PMV[1] of each (13818-2 7.6.3.1).
    pmv_forward: [(i32, i32); 2],
    pmv_backward: [(i32, i32); 2],
    /// Macroblocks skipped since the last coded one.
    skipped: u32,
    /// The last non-intra macroblock's prediction and vectors: a skipped macroblock repeats them (13818-2 7.6.6.3).
    last: Option<(vlc::BPrediction, (i32, i32), (i32, i32))>,
}

/// A 16 x 16 prediction and its two chroma blocks.
type Prediction = ([u8; 256], [u8; 64], [u8; 64]);

/// The average of two predictions, rounding up (13818-2 7.6.7.1).
fn average_prediction(a: &Prediction, b: &Prediction) -> Prediction {
    fn mean<const N: usize>(a: &[u8; N], b: &[u8; N]) -> [u8; N] {
        let mut out = [0u8; N];
        for ((slot, x), y) in out.iter_mut().zip(a).zip(b) {
            *slot = u8::try_from((u16::from(*x) + u16::from(*y) + 1) >> 1).unwrap_or(255);
        }
        out
    }
    (mean(&a.0, &b.0), mean(&a.1, &b.1), mean(&a.2, &b.2))
}

/// Codes the macroblock at `(x, y)` of a B picture from the past and future references: whichever of the forward,
/// backward and interpolated predictions is closest — each as one frame vector or, in an interlaced picture with the
/// field tools (M12.36), as two field vectors — intra when that is clearly cheaper, skipped when it repeats the last
/// macroblock's frame prediction with nothing to code. Returns its forward frame vector (half pixels).
#[allow(clippy::too_many_arguments)]
fn encode_b_macroblock(
    writer: &mut BitWriter,
    cur: &Frame420,
    (past, future, fields, temporal): (
        &Frame420,
        &Frame420,
        Option<&FieldSet>,
        ((i32, i32), (i32, i32)),
    ),
    recon: &mut Frame420,
    (x, y): (usize, usize),
    coding: Coding,
    predictors: &mut Predictors,
    (state, uses): (&mut BState, &mut FieldUse),
    can_skip: bool,
) -> (i32, i32) {
    use vlc::BPrediction;
    let (qs, f_code) = (coding.qs, coding.f_code);
    let search = |reference: &Frame420, pmv: (i32, i32), temporal: (i32, i32)| {
        let starts = [(pmv.0 / 2, pmv.1 / 2), (temporal.0 / 2, temporal.1 / 2)];
        let (dx, dy, sad) = motion::search_fast(cur, reference, (x, y), coding.search, &starts);
        if coding.half_pel {
            motion::refine_half(cur, reference, (x, y), (dx, dy, sad), coding.search)
        } else {
            (dx.saturating_mul(2), dy.saturating_mul(2), sad)
        }
    };
    let [forward_pmv, _] = state.pmv_forward;
    let [backward_pmv, _] = state.pmv_backward;
    let (fx, fy, sad_forward) = search(past, forward_pmv, temporal.0);
    let (bx, by, sad_backward) = search(future, backward_pmv, temporal.1);
    let forward = motion::predict(past, x, y, fx, fy);
    let backward = motion::predict(future, x, y, bx, by);
    let interpolated = average_prediction(&forward, &backward);
    // Two vectors cost more bits than one: the interpolated prediction must be a little better to be chosen.
    let sad_interpolated = motion::sad_prediction(cur, x, y, &interpolated.0).saturating_add(32);
    let mut candidates = vec![
        (BPrediction::Interpolated, false, sad_interpolated),
        (BPrediction::Forward, false, sad_forward),
        (BPrediction::Backward, false, sad_backward),
    ];
    // M12.36: the same three as field predictions, each field of the macroblock from either field of a reference.
    let field = fields.and_then(|set| {
        let future_fields = set.future.as_ref()?;
        let forward_motion = search_fields((&set.current, &set.past), (x, y), coding, (fx, fy));
        let backward_motion =
            search_fields((&set.current, future_fields), (x, y), coding, (bx, by));
        let forward_prediction = predict_fields(&set.past, (x, y), &forward_motion);
        let backward_prediction = predict_fields(future_fields, (x, y), &backward_motion);
        let both = average_prediction(&forward_prediction, &backward_prediction);
        let sad_both = motion::sad_prediction(cur, x, y, &both.0);
        Some((
            (forward_motion, forward_prediction),
            (backward_motion, backward_prediction),
            (both, sad_both),
        ))
    });
    if let Some(((forward_motion, _), (backward_motion, _), (_, sad_both))) = &field {
        let penalty = u32::from(qs).saturating_mul(FIELD_MOTION_PENALTY);
        candidates.extend([
            (
                BPrediction::Interpolated,
                true,
                sad_both
                    .saturating_add(32)
                    .saturating_add(penalty.saturating_mul(2)),
            ),
            (
                BPrediction::Forward,
                true,
                forward_motion.sad.saturating_add(penalty),
            ),
            (
                BPrediction::Backward,
                true,
                backward_motion.sad.saturating_add(penalty),
            ),
        ]);
    }
    let (mode, by_fields, sad) = candidates
        .into_iter()
        .min_by_key(|(_, _, sad)| *sad)
        .unwrap_or((BPrediction::Forward, false, sad_forward));

    if motion::intra_cost(cur, x, y).saturating_add(512) < sad {
        vlc::write_macroblock_address_increment_after(writer, state.skipped);
        state.skipped = 0;
        vlc::write_b_macroblock_type(writer, vlc::BType::Intra);
        let field_dct = coding.field && intra_field_dct(cur, x, y, qs);
        if coding.field {
            writer.write_bits(u32::from(field_dct), 1); // dct_type
        }
        if !state.prev_intra {
            *predictors = Predictors::new();
        }
        encode_intra_blocks(writer, cur, recon, x, y, qs, field_dct, predictors);
        state.prev_intra = true;
        state.pmv_forward = [(0, 0); 2];
        state.pmv_backward = [(0, 0); 2];
        state.last = None;
        return (0, 0);
    }

    let (vector_forward, vector_backward) = ((fx, fy), (bx, by));
    let field = field.filter(|_| by_fields);
    let prediction = match (&field, mode) {
        (Some(((_, prediction), _, _)), BPrediction::Forward)
        | (Some((_, (_, prediction), _)), BPrediction::Backward)
        | (Some((_, _, (prediction, _))), BPrediction::Interpolated) => *prediction,
        (None, BPrediction::Forward) => forward,
        (None, BPrediction::Backward) => backward,
        (None, BPrediction::Interpolated) => interpolated,
    };
    let levels = quantise_inter(cur, (x, y), &prediction, qs, coding.field);

    // Only a frame prediction is repeated by a skipped macroblock here: one after a field-predicted macroblock is coded.
    let repeats = field.is_none() && state.last == Some((mode, vector_forward, vector_backward));
    if can_skip && levels.cbp == 0 && !state.prev_intra && repeats {
        // Skipped: the decoder repeats the last macroblock's prediction and vectors; the vector predictors stay.
        state.skipped = state.skipped.saturating_add(1);
    } else {
        vlc::write_macroblock_address_increment_after(writer, state.skipped);
        state.skipped = 0;
        vlc::write_b_macroblock_type(writer, vlc::BType::Mc(mode, levels.cbp != 0));
        if coding.field {
            // frame_motion_type: field-based (01) or frame-based (10) prediction (13818-2 table 6-17); then dct_type
            // when coded.
            writer.write_bits(if field.is_some() { 1 } else { 2 }, 2);
            if levels.cbp != 0 {
                writer.write_bits(u32::from(levels.field_dct), 1);
            }
            uses.note(field.is_some() || (levels.cbp != 0 && levels.field_dct));
        }
        // The forward vectors first, then the backward ones (13818-2 6.2.5.2).
        let uses = [
            (mode != BPrediction::Backward, vector_forward, 0),
            (mode != BPrediction::Forward, vector_backward, 1),
        ];
        for (used, vector, direction) in uses {
            if !used {
                continue;
            }
            let pmv = if direction == 0 {
                &mut state.pmv_forward
            } else {
                &mut state.pmv_backward
            };
            match &field {
                Some(((forward_motion, _), (backward_motion, _), _)) => {
                    let motion = if direction == 0 {
                        forward_motion
                    } else {
                        backward_motion
                    };
                    write_field_vectors(writer, motion, pmv, f_code);
                }
                None => {
                    let [first, _] = *pmv;
                    let delta_x = vlc::wrap_motion_delta(vector.0 - first.0, f_code);
                    let delta_y = vlc::wrap_motion_delta(vector.1 - first.1, f_code);
                    vlc::write_motion_delta(writer, delta_x, f_code);
                    vlc::write_motion_delta(writer, delta_y, f_code);
                    *pmv = [vector; 2];
                }
            }
        }
        if levels.cbp != 0 {
            vlc::write_coded_block_pattern(writer, reverse_cbp(levels.cbp));
        }
    }
    write_inter_blocks(writer, recon, (x, y), &prediction, &levels, qs);
    state.prev_intra = false;
    state.last = field
        .is_none()
        .then_some((mode, vector_forward, vector_backward));
    vector_forward
}

/// Writes a field prediction's two vectors, each after its field select bit, from their predictors `pmv` (the vertical
/// part predicted from half its predictor), and keeps them as the predictors, the vertical part doubled (13818-2
/// 7.6.3.1).
fn write_field_vectors(
    writer: &mut BitWriter,
    motion: &FieldMotion,
    pmv: &mut [(i32, i32); 2],
    f_code: u8,
) {
    for ((select, (vx, vy)), predictor) in
        motion.select.iter().zip(motion.vectors).zip(pmv.iter_mut())
    {
        writer.write_bits(u32::try_from(*select).unwrap_or(0), 1);
        let delta_x = vlc::wrap_motion_delta(vx - predictor.0, f_code);
        let delta_y = vlc::wrap_motion_delta(vy - (predictor.1 >> 1), f_code);
        vlc::write_motion_delta(writer, delta_x, f_code);
        vlc::write_motion_delta(writer, delta_y, f_code);
        *predictor = (vx, vy.saturating_mul(2));
    }
}

/// A non-intra macroblock's quantised residual: its four luma blocks (as fields when they are cheaper so, with
/// `field`), its two chroma blocks and its coded block pattern (bit i: block i).
struct InterLevels {
    field_dct: bool,
    luma: [[i16; 64]; 4],
    cb: [i16; 64],
    cr: [i16; 64],
    cbp: u8,
}

/// Quantises the residual of the macroblock at `(x, y)` from `prediction`.
fn quantise_inter(
    cur: &Frame420,
    (x, y): (usize, usize),
    prediction: &Prediction,
    qs: u16,
    field: bool,
) -> InterLevels {
    let w = usize::from(cur.width);
    let cw = w / 2;
    let quantised = |field: bool| {
        luma_layout(field, w, (x, y)).map(|((pw, bx, by), (ps, px, py))| {
            quantise_residual_block(&cur.y, pw, bx, by, &prediction.0, ps, px, py, qs)
        })
    };
    let frame_levels = quantised(false);
    let (field_dct, luma) = if field {
        let field_levels = quantised(true);
        if levels_cost(&field_levels) < levels_cost(&frame_levels) {
            (true, field_levels)
        } else {
            (false, frame_levels)
        }
    } else {
        (false, frame_levels)
    };
    let cb = quantise_residual_block(&cur.cb, cw, x / 2, y / 2, &prediction.1, 8, 0, 0, qs);
    let cr = quantise_residual_block(&cur.cr, cw, x / 2, y / 2, &prediction.2, 8, 0, 0, qs);
    let mut cbp = 0u8;
    for (index, block) in luma.iter().chain([&cb, &cr]).enumerate() {
        if block.iter().any(|&level| level != 0) {
            cbp |= 1 << index;
        }
    }
    InterLevels {
        field_dct,
        luma,
        cb,
        cr,
        cbp,
    }
}

/// Writes the coded blocks of a non-intra macroblock and reconstructs it from `prediction` into `recon`.
fn write_inter_blocks(
    writer: &mut BitWriter,
    recon: &mut Frame420,
    (x, y): (usize, usize),
    prediction: &Prediction,
    levels: &InterLevels,
    qs: u16,
) {
    let w = usize::from(recon.width);
    let cw = w / 2;
    let layout = luma_layout(levels.field_dct, w, (x, y));
    for (index, (((pw, bx, by), (ps, px, py)), block)) in
        layout.iter().zip(&levels.luma).enumerate()
    {
        let coded = levels.cbp & (1 << index) != 0;
        if coded {
            write_non_intra_block(writer, block);
        }
        reconstruct_inter_block(
            &mut recon.y,
            *pw,
            *bx,
            *by,
            &prediction.0,
            *ps,
            *px,
            *py,
            coded.then_some(block),
            qs,
        );
    }
    for (bit, block, plane, pred) in [
        (16u8, &levels.cb, &mut recon.cb, &prediction.1),
        (32u8, &levels.cr, &mut recon.cr, &prediction.2),
    ] {
        let coded = levels.cbp & bit != 0;
        if coded {
            write_non_intra_block(writer, block);
        }
        reconstruct_inter_block(
            plane,
            cw,
            x / 2,
            y / 2,
            pred,
            8,
            0,
            0,
            coded.then_some(block),
            qs,
        );
    }
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

/// The AC coefficients of an intra block, in table one (`intra_vlc_format` is 1 in every picture).
fn write_intra_ac(writer: &mut BitWriter, levels: &[i16; 64]) {
    write_coefficients(writer, levels, 1, vlc::DctTable::One);
}

/// The coefficients of a non-intra block, in table zero.
fn write_non_intra_block(writer: &mut BitWriter, levels: &[i16; 64]) {
    write_coefficients(writer, levels, 0, vlc::DctTable::Zero);
}

/// Writes the nonzero levels from scan position `from` on as run/level codes of `table`, then end of block.
fn write_coefficients(
    writer: &mut BitWriter,
    levels: &[i16; 64],
    from: usize,
    table: vlc::DctTable,
) {
    let mut run = 0u8;
    let mut first = true;

    for &level in levels.iter().skip(from) {
        if level == 0 {
            run = run.saturating_add(1);
            continue;
        }
        vlc::write_coefficient(writer, table, run, i32::from(level), first && from == 0);
        run = 0;
        first = false;
    }

    vlc::write_end_of_block_in(writer, table);
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

#[cfg(test)]
mod tests {
    use super::{empty_frame, encode_slices};
    use crate::Frame420;

    /// A picture with detail everywhere: a moving pattern of `phase`.
    fn picture(phase: usize) -> Frame420 {
        let mut frame = empty_frame(720, 480);
        for (index, sample) in frame.y.iter_mut().enumerate() {
            let (x, y) = (index % 720, index / 720);
            *sample =
                u8::try_from((x * 3 + y * 5 + phase * 7 + (x * y) % 31) % 220 + 16).unwrap_or(16);
        }
        for (index, sample) in frame.cb.iter_mut().enumerate() {
            *sample = u8::try_from((index + phase) % 200 + 28).unwrap_or(128);
        }
        for (index, sample) in frame.cr.iter_mut().enumerate() {
            *sample = u8::try_from((index * 3 + phase) % 200 + 28).unwrap_or(128);
        }
        frame
    }

    #[test]
    fn slices_on_several_threads_are_the_same_as_on_one() -> Result<(), crate::EncodeError> {
        let first = picture(0);
        let second = picture(3);
        let encode = |frame: &Frame420, reference: Option<&Frame420>, workers: usize| {
            let mut recon = empty_frame(frame.width, frame.height);
            let mut out = Vec::new();
            let coding = super::Coding {
                code: 8,
                qs: 16,
                f_code: 2,
                search: 15,
                half_pel: true,
                field: false,
                between: (0, 0),
            };
            let refs =
                reference.map_or(super::Refs::None, |frame| super::Refs::Forward(frame, None));
            encode_slices(frame, (refs, &[]), &mut recon, (coding, workers), &mut out)
                .map(|_| (out, recon))
        };
        let (intra, intra_recon) = encode(&first, None, 1)?;
        for workers in [2, 7, 30] {
            let (bytes, recon) = encode(&first, None, workers)?;
            assert_eq!(bytes, intra, "intra, {workers} threads");
            assert_eq!(recon, intra_recon, "intra recon, {workers} threads");
        }
        let (inter, inter_recon) = encode(&second, Some(&intra_recon), 1)?;
        for workers in [2, 7, 30] {
            let (bytes, recon) = encode(&second, Some(&intra_recon), workers)?;
            assert_eq!(bytes, inter, "P, {workers} threads");
            assert_eq!(recon, inter_recon, "P recon, {workers} threads");
        }
        Ok(())
    }
}
