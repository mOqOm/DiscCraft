//! HD transcode for Blu-ray.
//!
//! Decodes a supported media file, scales it to an HD frame, and encodes an elementary stream plus an access-unit
//! index: Annex B H.264, or MPEG-2 MP@HL ([`HdCodec::Mpeg2`]: the interlaced 1920 x 1080 formats, 59.94i / 50i,
//! and since M10.39 the progressive ones, 1080p and 720p). The functions and files named `h264` make either, as the
//! options say.

use std::convert::TryFrom;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use craft_video::h264::{
    Encoder, EncoderConfig, Packet, PacketFormat, Pass, PassStats, Preset, Profile, RateControl,
    YuvFrame,
};
use filmcraft_frame::VideoFrame;
use filmcraft_time::Tick;

use crate::source::Source;
use crate::{TICKS_PER_SECOND, TranscodeError, io_error};

/// HD encoding settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdOptions {
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Frame-rate numerator.
    pub fps_num: u32,
    /// Frame-rate denominator.
    pub fps_den: u32,
    /// Target bitrate in kilobits per second.
    pub target_kbps: u32,
    /// Maximum bitrate in kilobits per second.
    pub max_kbps: u32,
    /// The video coding.
    pub codec: HdCodec,
    /// How hard the MPEG-2 encoder searches for motion: Encore's quality (1 to 5; its "Long Form" MPEG-2
    /// presets' 3, the others' 5). The H.264 encoder has no such setting.
    pub quality: u8,
    /// How the pictures are made (Encore's rendering checkboxes).
    pub rendering: crate::Rendering,
    /// How the bit rate is kept: one-pass VBR (Encore's Blu-ray presets), two-pass VBR (the video is gone through
    /// twice, the first time to measure it) or CBR (the target is also the peak).
    pub mode: crate::RateMode,
    /// H.264 of an interlaced format (1080i, M12.28): the frames are coded as interlaced frames, the top field
    /// first (craft-video's `EncoderConfig::interlaced`). MPEG-2 HD is always signalled interlaced.
    pub interlaced: bool,
    /// A standard-definition frame (720 x 480 or 720 x 576: an SD Blu-ray's H.264, M10.41) shown 16:9, else 4:3: the
    /// shape of its pixels ([`output_pixel_aspect`]). An HD frame's is fixed by its size.
    pub wide: bool,
}

/// The video coding of an HD stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HdCodec {
    /// H.264 High Profile, progressive frames.
    #[default]
    H264,
    /// MPEG-2 MP@HL: 1920 x 1080 at 29.97 or 25 frames a second, signalled as interlaced (59.94i / 50i), each frame
    /// coded as one frame picture whose two fields are from the same moment, or (M12.37) from an interlaced source
    /// at that rate, of the source's two moments with the field tools; or (M10.39) a progressive sequence of
    /// 1080 lines at 23.976 or 24, or of 720 lines at 23.976, 24, 50 or 59.94 frames a second.
    Mpeg2,
    /// M12.43: H.265 / HEVC Main 10 (Ultra HD Blu-ray's codec, 3840 x 2160 or 1920 x 1080), progressive frames: I and
    /// P pictures (craft-video's encoder does not reorder), an IDR picture each second at most (the format's 1.001 s),
    /// BT.709 colour, the 8-bit pictures carried at 10 bits. Annex B with an access unit delimiter before every
    /// picture and the parameter sets before every IDR picture. One pass (a two-pass encode is made in one).
    Hevc,
}

/// One coded picture from either encoder, its times counted in frames.
struct Coded {
    data: Vec<u8>,
    pts: i64,
    dts: i64,
    keyframe: bool,
}

impl From<Packet> for Coded {
    fn from(packet: Packet) -> Self {
        Self {
            data: packet.data,
            pts: packet.pts,
            dts: packet.dts,
            keyframe: packet.keyframe,
        }
    }
}

/// M12.40 / M12.44: a hardware video encoder from outside this crate (the system's: the applications register it
/// with [`set_hardware_encoder`], the `platform` crate making it). It takes pictures in its codec's layout
/// ([`HardwareCodec`]) and gives Annex B access units in decoding order, without B pictures.
pub trait HardwareVideo {
    /// Encodes `picture` shown at `pts` (100 ns units): the access units it has ready.
    ///
    /// # Errors
    ///
    /// When the encoder fails.
    fn encode(&mut self, picture: &[u8], pts: i64) -> Result<Vec<HardwarePacket>, String>;

    /// The access units still held back, at the end.
    ///
    /// # Errors
    ///
    /// When the encoder fails.
    fn finish(&mut self) -> Result<Vec<HardwarePacket>, String>;
}

/// One access unit from a [`HardwareVideo`] encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwarePacket {
    /// The access unit, start code prefixed (H.264: with its access unit delimiter; HEVC: the transcode adds the
    /// delimiter and the parameter sets where they are missing).
    pub data: Vec<u8>,
    /// Its presentation time, 100 ns units, as given.
    pub pts: i64,
    /// Whether it starts a GOP.
    pub key: bool,
}

/// The stream a [`HardwareVideo`] encoder makes, and the pictures it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareCodec {
    /// Blu-ray's H.264 (High profile, level 4.1) from NV12 pictures: the luma, then the chroma samples interleaved.
    H264,
    /// HEVC Main 10 (M12.44) from P010 pictures: as NV12, but each sample two bytes, little-endian, its 10 bits at
    /// the top.
    HevcMain10,
}

/// What an HD stream asks of a [`HardwareVideo`] encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareConfig {
    /// The stream to make.
    pub codec: HardwareCodec,
    /// Picture width.
    pub width: u32,
    /// Picture height.
    pub height: u32,
    /// Frame rate numerator.
    pub fps_num: u32,
    /// Frame rate denominator.
    pub fps_den: u32,
    /// Average bit rate, bit/s.
    pub bitrate: u32,
    /// Peak bit rate, bit/s.
    pub max_bitrate: u32,
    /// Constant bit rate (the average is the peak).
    pub cbr: bool,
    /// Frames from one key frame to the next.
    pub gop: u32,
    /// Slices a picture (H.264).
    pub slices: u32,
    /// The shape of a pixel (the VUI's sample aspect ratio).
    pub sar: (u32, u32),
}

/// Makes a [`HardwareVideo`] encoder for an HD stream, or says why it cannot.
pub type HardwareFactory = fn(&HardwareConfig) -> Result<Box<dyn HardwareVideo>, String>;

static HARDWARE: std::sync::RwLock<Option<HardwareFactory>> = std::sync::RwLock::new(None);

/// Uses `factory`'s hardware encoders for the HD H.264 (M12.40) and HEVC (M12.44) the transcodes make, or none
/// (`None`, the default): progressive formats in one pass (VBR or CBR; HEVC is always one pass); an interlaced format
/// and a two-pass H.264 encode stay on the CPU, and so does a video whose hardware encode cannot start or fails (it
/// is encoded again on the CPU).
pub fn set_hardware_encoder(factory: Option<HardwareFactory>) {
    if let Ok(mut slot) = HARDWARE.write() {
        *slot = factory;
    }
}

fn hardware_encoder() -> Option<HardwareFactory> {
    HARDWARE.read().ok().and_then(|slot| *slot)
}

/// The hardware encoder's settings for `options`: the CPU's GOP, slices and pixel shape; 90 % of the target as the
/// average (a hardware encoder overshot it by about a tenth on detailed pictures, M12.39).
fn hardware_config(options: &HdOptions, codec: HardwareCodec) -> HardwareConfig {
    let cbr = options.mode == crate::RateMode::Cbr;
    let target = options.target_kbps.saturating_mul(1000);
    HardwareConfig {
        codec,
        width: options.width,
        height: options.height,
        fps_num: options.fps_num,
        fps_den: options.fps_den,
        bitrate: if cbr { target } else { target / 10 * 9 },
        max_bitrate: if cbr {
            target
        } else {
            options.max_kbps.saturating_mul(1000)
        },
        cbr,
        gop: keyint(options),
        slices: match codec {
            HardwareCodec::H264 => 4,
            HardwareCodec::HevcMain10 => 1,
        },
        sar: output_pixel_aspect(options.width, options.height, options.wide),
    }
}

/// A hardware encoder at work and where it is: one frame in 100 ns units, and the pictures given out so far; for
/// HEVC (M12.44) the last parameter sets it gave (Annex B), to repeat before an IDR picture without them.
struct HardwareState {
    encoder: Box<dyn HardwareVideo>,
    codec: HardwareCodec,
    frame: i64,
    given: i64,
    last_pts: Option<i64>,
    sets: Vec<u8>,
}

impl HardwareState {
    /// `packets` as coded units in frames, decoded in the order they come (no B pictures: each as shown).
    fn coded(&mut self, packets: Vec<HardwarePacket>) -> Result<Vec<Coded>, TranscodeError> {
        packets
            .into_iter()
            .map(|packet| {
                let pts = packet
                    .pts
                    .checked_add(self.frame / 2)
                    .and_then(|time| time.checked_div(self.frame))
                    .ok_or_else(|| TranscodeError::Encode("a hardware picture time".to_string()))?;
                if self.last_pts.is_some_and(|last| pts <= last) {
                    return Err(TranscodeError::Encode(
                        "the hardware encoder reordered the pictures".to_string(),
                    ));
                }
                self.last_pts = Some(pts);
                self.given = self.given.saturating_add(1);
                let data = match self.codec {
                    HardwareCodec::H264 => packet.data,
                    HardwareCodec::HevcMain10 => {
                        hevc_hardware_unit(&packet.data, packet.key, &mut self.sets)?
                    }
                };
                Ok(Coded {
                    data,
                    pts,
                    dts: pts,
                    keyframe: packet.key,
                })
            })
            .collect()
    }
}

/// `planes` as one NV12 picture `width` x `height`.
fn nv12_of(planes: &YuvPlanes, (width, height): (usize, usize)) -> Result<Vec<u8>, TranscodeError> {
    let wrong =
        || TranscodeError::Encode("a picture the wrong size for the hardware encoder".to_string());
    let mut nv12 = Vec::with_capacity(
        width
            .checked_mul(height)
            .ok_or_else(wrong)?
            .saturating_mul(3)
            / 2,
    );
    for row in 0..height {
        let start = row.checked_mul(planes.y_stride).ok_or_else(wrong)?;
        nv12.extend_from_slice(
            planes
                .y
                .get(start..start.checked_add(width).ok_or_else(wrong)?)
                .ok_or_else(wrong)?,
        );
    }
    let half = width / 2;
    for row in 0..height / 2 {
        let start = row.checked_mul(planes.uv_stride).ok_or_else(wrong)?;
        let end = start.checked_add(half).ok_or_else(wrong)?;
        let (u, v) = (
            planes.u.get(start..end).ok_or_else(wrong)?,
            planes.v.get(start..end).ok_or_else(wrong)?,
        );
        for (cb, cr) in u.iter().zip(v) {
            nv12.push(*cb);
            nv12.push(*cr);
        }
    }
    Ok(nv12)
}

/// The encoder of an HD stream: H.264, or MPEG-2 for the interlaced formats (with what each picture took, for a
/// first pass of two), or (M12.40) a hardware H.264 encoder.
enum HdEncoder {
    H264(Box<Encoder>),
    /// M12.43: the encoder, and its parameter sets as Annex B (VPS, SPS, PPS).
    Hevc(Box<craft_video::hevc::Encoder>, Vec<u8>),
    Hardware(HardwareState),
    Mpeg2(
        Box<disccraft_mpeg2venc::encoder::Encoder>,
        Vec<disccraft_mpeg2venc::encoder::PictureStats>,
    ),
}

/// Which pass of a two-pass encode an [`HdEncoder`] makes (M6.328).
enum HdPass {
    /// The only one.
    Single,
    /// The first of two: it measures the pictures; what it writes is not kept.
    First,
    /// The second, from what the first measured.
    Second(HdStats),
}

/// What a first pass measured.
enum HdStats {
    H264(PassStats),
    Mpeg2(Vec<disccraft_mpeg2venc::encoder::PictureStats>),
}

/// The MPEG-2 encoder's settings for `options`, and the rate of a progressive sequence: 1920 x 1080, or (M6.393)
/// HDV's 1440 x 1080, as 59.94i / 50i (29.97 or 25 frames a second) or (M10.39) progressive at 23.976 or 24, and
/// 1280 x 720 progressive at 23.976, 24, 50 or 59.94.
fn mpeg2_config(
    options: &HdOptions,
) -> Result<
    (
        disccraft_mpeg2venc::encoder::EncoderConfig,
        Option<disccraft_mpeg2venc::encoder::FrameRate>,
    ),
    TranscodeError,
> {
    use disccraft_mpeg2venc::encoder::{Format, FrameRate};
    let unsupported = || {
        TranscodeError::Unsupported(
            "MPEG-2 HD is 1080 lines as 59.94i, 50i, 23.976p or 24p, or 720 lines as 23.976p, 24p, 50p or 59.94p"
                .to_string(),
        )
    };
    let rate = FrameRate::of(options.fps_num, options.fps_den).ok_or_else(unsupported)?;
    let (format, progressive) = match (options.width, options.height, rate) {
        (1920 | 1440, 1080, FrameRate::Fps29_97 | FrameRate::Fps25) => (Format::Hd1080, None),
        (1920 | 1440, 1080, FrameRate::Fps23_976 | FrameRate::Fps24) => {
            (Format::Hd1080, Some(rate))
        }
        (
            1280,
            720,
            FrameRate::Fps23_976 | FrameRate::Fps24 | FrameRate::Fps50 | FrameRate::Fps59_94,
        ) => (Format::Hd720, Some(rate)),
        _ => return Err(unsupported()),
    };
    let format = if options.width == 1440 {
        Format::Hd1440
    } else {
        format
    };
    let standard = if matches!(rate, FrameRate::Fps25 | FrameRate::Fps50) {
        disccraft_mpeg2venc::Standard::Pal
    } else {
        disccraft_mpeg2venc::Standard::Ntsc
    };
    // Half a second, as on a DVD (a whole number of I B B P groups for a progressive rate).
    let gop_size = match progressive {
        Some(rate) => u16::try_from(rate.nominal() / 2 / 3 * 3).unwrap_or(12),
        None if standard == disccraft_mpeg2venc::Standard::Ntsc => 15,
        None => 12,
    };
    let config = disccraft_mpeg2venc::encoder::EncoderConfig {
        standard,
        gop_size,
        bitrate: options.target_kbps.saturating_mul(1000),
        // CBR: the target is also the peak.
        max_bitrate: if options.mode == crate::RateMode::Cbr {
            options.target_kbps.saturating_mul(1000)
        } else {
            options.max_kbps.saturating_mul(1000)
        },
        wide: true,
        format,
        pulldown: false,
    };
    Ok((config, progressive))
}

/// The MPEG-2 encoder for `options` ([`mpeg2_config`]): a progressive sequence at a progressive format's rate.
fn mpeg2_encoder(
    options: &HdOptions,
) -> Result<disccraft_mpeg2venc::encoder::Encoder, TranscodeError> {
    let (config, progressive) = mpeg2_config(options)?;
    let mut encoder = disccraft_mpeg2venc::encoder::Encoder::new(config)
        .map_err(|error| TranscodeError::Encode(error.to_string()))?;
    if let Some(rate) = progressive {
        encoder
            .set_progressive(rate)
            .map_err(|error| TranscodeError::Encode(error.to_string()))?;
    }
    Ok(encoder)
}

/// `planes` (1920 x 1080) as the MPEG-2 encoder's frame of the coded size (1920 x 1088): the last line repeated
/// below the picture, which codes to almost nothing.
fn mpeg2_frame(
    planes: &YuvPlanes,
    coded: (u16, u16),
) -> Result<disccraft_mpeg2venc::Frame420, TranscodeError> {
    let pad = |plane: &[u8], stride: usize, width: usize, height: usize| -> Option<Vec<u8>> {
        if stride == 0 || width > stride {
            return None;
        }
        let mut out = Vec::with_capacity(width.checked_mul(height)?);
        let rows = plane.len() / stride;
        for row in 0..height {
            let from = row.min(rows.checked_sub(1)?).checked_mul(stride)?;
            out.extend_from_slice(plane.get(from..from.checked_add(width)?)?);
        }
        Some(out)
    };
    let (width, height) = (usize::from(coded.0), usize::from(coded.1));
    let planes = (|| {
        Some((
            pad(&planes.y, planes.y_stride, width, height)?,
            pad(&planes.u, planes.uv_stride, width / 2, height / 2)?,
            pad(&planes.v, planes.uv_stride, width / 2, height / 2)?,
        ))
    })();
    let (y, cb, cr) =
        planes.ok_or_else(|| TranscodeError::Unsupported("picture too small".to_string()))?;
    Ok(disccraft_mpeg2venc::Frame420 {
        width: coded.0,
        height: coded.1,
        y,
        cb,
        cr,
    })
}

impl HdEncoder {
    fn new(options: &HdOptions) -> Result<Self, TranscodeError> {
        Self::with_pass(options, HdPass::Single, None)
    }

    /// The registered hardware encoder for a video's progressive H.264 (M12.40) or HEVC (M12.44) in one pass; `None`
    /// when there is none, the stream is not one it makes, or it cannot start.
    fn hardware(options: &HdOptions, pass: &HdPass) -> Option<Self> {
        let factory = hardware_encoder()?;
        let codec = match options.codec {
            HdCodec::H264 if !options.interlaced => HardwareCodec::H264,
            HdCodec::Hevc => HardwareCodec::HevcMain10,
            HdCodec::H264 | HdCodec::Mpeg2 => return None,
        };
        if !matches!(pass, HdPass::Single) {
            return None;
        }
        let encoder = factory(&hardware_config(options, codec)).ok()?;
        let frame = i64::from(options.fps_den)
            .checked_mul(10_000_000)?
            .checked_div(i64::from(options.fps_num))?;
        Some(Self::Hardware(HardwareState {
            encoder,
            codec,
            frame: frame.max(1),
            given: 0,
            last_pts: None,
            sets: Vec::new(),
        }))
    }

    /// An encoder for `pass` of a two-pass encode; `fields`: of pictures whose two fields were taken at different
    /// moments, the top one first or not ([`kept_fields`]), which MPEG-2 codes with its field tools (M12.36) and both
    /// codecs mark in that order (M12.42: a source whose bottom field comes first stays so). H.264 codes the
    /// interlaced formats' frames as interlaced frames either way.
    fn with_pass(
        options: &HdOptions,
        pass: HdPass,
        fields: Option<bool>,
    ) -> Result<Self, TranscodeError> {
        match options.codec {
            HdCodec::H264 => {
                let mut config = encoder_config(options);
                if fields == Some(false) {
                    config.interlaced = Some(craft_video::h264::FieldOrder::BottomFirst);
                }
                config.pass = match pass {
                    HdPass::Single | HdPass::Second(HdStats::Mpeg2(_)) => Pass::Single,
                    HdPass::First => Pass::First,
                    HdPass::Second(HdStats::H264(stats)) => Pass::Second(stats),
                };
                Encoder::new(config)
                    .map(|encoder| Self::H264(Box::new(encoder)))
                    .map_err(|error| TranscodeError::Encode(error.to_string()))
            }
            HdCodec::Hevc => {
                let mut config = craft_video::hevc::EncoderConfig::new(
                    options.width,
                    options.height,
                    options.fps_num,
                    options.fps_den,
                );
                config.profile = craft_video::hevc::Profile::Main10;
                config.rate = craft_video::hevc::RateControl::Bitrate {
                    kbps: options.target_kbps,
                };
                config.keyint = keyint(options);
                let encoder = craft_video::hevc::Encoder::new(config)
                    .map_err(|error| TranscodeError::Encode(error.to_string()))?;
                let mut sets = Vec::new();
                for set in encoder.parameter_sets() {
                    sets.extend_from_slice(&[0, 0, 0, 1]);
                    sets.extend_from_slice(&set);
                }
                Ok(Self::Hevc(Box::new(encoder), sets))
            }
            HdCodec::Mpeg2 => {
                let mut encoder = mpeg2_encoder(options)?;
                encoder.set_quality(options.quality);
                encoder.set_b_frames(crate::picture::B_FRAMES);
                if let Some(top_first) = fields {
                    encoder.set_interlaced(top_first);
                }
                if let HdPass::Second(HdStats::Mpeg2(first)) = &pass {
                    encoder.plan_second_pass(first);
                }
                Ok(Self::Mpeg2(Box::new(encoder), Vec::new()))
            }
        }
    }

    /// What a first pass measured.
    fn stats(&self) -> Option<HdStats> {
        match self {
            Self::H264(encoder) => encoder.pass_stats().map(HdStats::H264),
            Self::Hardware(_) | Self::Hevc(..) => None,
            Self::Mpeg2(_, pictures) => Some(HdStats::Mpeg2(pictures.clone())),
        }
    }

    /// Encodes picture `index`: the pictures that are ready (H.264 holds some back to reorder them).
    fn encode(
        &mut self,
        planes: &YuvPlanes,
        index: i64,
        options: &HdOptions,
    ) -> Result<Vec<Coded>, TranscodeError> {
        match self {
            Self::H264(encoder) => Ok(encoder
                .encode(&planes.frame(), index)
                .map_err(|error| TranscodeError::Encode(error.to_string()))?
                .into_iter()
                .map(Coded::from)
                .collect()),
            Self::Hevc(encoder, sets) => {
                let (y, u, v) = (ten_bit(&planes.y), ten_bit(&planes.u), ten_bit(&planes.v));
                let packet = encoder
                    .encode(&craft_video::hevc::Frame {
                        y: &y,
                        u: &u,
                        v: &v,
                        y_stride: planes.y_stride,
                        uv_stride: planes.uv_stride,
                    })
                    .map_err(|error| TranscodeError::Encode(error.to_string()))?;
                let data = hevc_annex_b(&packet.data, packet.keyframe.then_some(sets.as_slice()))?;
                Ok(vec![Coded {
                    data,
                    pts: index,
                    dts: index,
                    keyframe: packet.keyframe,
                }])
            }
            Self::Hardware(state) => {
                let size = (
                    usize::try_from(options.width).unwrap_or(0),
                    usize::try_from(options.height).unwrap_or(0),
                );
                let picture = match state.codec {
                    HardwareCodec::H264 => nv12_of(planes, size)?,
                    HardwareCodec::HevcMain10 => p010_of(&nv12_of(planes, size)?),
                };
                let pts = index.saturating_mul(state.frame);
                let packets = state
                    .encoder
                    .encode(&picture, pts)
                    .map_err(TranscodeError::Encode)?;
                state.coded(packets)
            }
            Self::Mpeg2(encoder, pictures) => {
                let (config, _) = mpeg2_config(options)?;
                let frame = mpeg2_frame(planes, config.format.coded_size(config.standard))?;
                // A GOP every `gop_size` frames, as `encode_frame` starts them.
                let start = index.rem_euclid(i64::from(config.gop_size.max(1))) == 0;
                let coded = encoder
                    .push_frame(&frame, start)
                    .map_err(|error| TranscodeError::Encode(error.to_string()))?;
                Ok(mpeg2_coded(coded, pictures))
            }
        }
    }

    /// The pictures still held back.
    fn flush(&mut self) -> Result<Vec<Coded>, TranscodeError> {
        match self {
            Self::H264(encoder) => Ok(encoder.flush().into_iter().map(Coded::from).collect()),
            // No pictures are held back.
            Self::Hevc(..) => Ok(Vec::new()),
            Self::Hardware(state) => {
                let packets = state.encoder.finish().map_err(TranscodeError::Encode)?;
                state.coded(packets)
            }
            Self::Mpeg2(encoder, pictures) => {
                let coded = encoder
                    .flush()
                    .map_err(|error| TranscodeError::Encode(error.to_string()))?;
                Ok(mpeg2_coded(coded, pictures))
            }
        }
    }
}

/// MPEG-2 pictures in decoding order as coded units (frame numbers): shown at their place in display order, decoded
/// one frame before their place in decoding order (with B pictures a reference picture is decoded a frame before
/// the B pictures shown ahead of it; I and P pictures alone are decoded as shown). Their statistics go to `stats`
/// for a first pass of two.
fn mpeg2_coded(
    coded: Vec<disccraft_mpeg2venc::encoder::CodedPicture>,
    stats: &mut Vec<disccraft_mpeg2venc::encoder::PictureStats>,
) -> Vec<Coded> {
    coded
        .into_iter()
        .map(|picture| {
            let decoding = i64::try_from(stats.len()).unwrap_or(i64::MAX);
            stats.push(picture.stats);
            let pts = i64::try_from(picture.display).unwrap_or(i64::MAX);
            let dts = if crate::picture::B_FRAMES > 0 {
                decoding - 1
            } else {
                pts
            };
            Coded {
                data: picture.data,
                pts,
                dts,
                keyframe: picture.gop_start,
            }
        })
        .collect()
}

/// 8-bit samples as 10-bit ones (M12.43: the pictures are made at 8 bits; Main 10 carries them shifted up).
fn ten_bit(samples: &[u8]) -> Vec<u16> {
    samples
        .iter()
        .map(|sample| u16::from(*sample) << 2)
        .collect()
}

/// An HEVC access unit as Annex B (M12.43): an access unit delimiter (H.265 7.3.2.5, pic_type 2: any slice type),
/// `parameter_sets` (already Annex B) before an IDR picture, then each NAL unit of `coded` (each prefixed by a 4-byte
/// length, ISO/IEC 14496-15) after a start code.
fn hevc_annex_b(coded: &[u8], parameter_sets: Option<&[u8]>) -> Result<Vec<u8>, TranscodeError> {
    let broken = || TranscodeError::Encode("a malformed HEVC access unit".to_string());
    let mut out = Vec::with_capacity(coded.len().saturating_add(64));
    // nal_unit_type 35 (AUD), layer 0, temporal id 1; pic_type 2 and the RBSP stop bit.
    out.extend_from_slice(&[0, 0, 0, 1, 0x46, 0x01, 0x50]);
    if let Some(sets) = parameter_sets {
        out.extend_from_slice(sets);
    }
    let mut at = 0usize;
    while at < coded.len() {
        let length: [u8; 4] = coded
            .get(at..at.checked_add(4).ok_or_else(broken)?)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(broken)?;
        let start = at.checked_add(4).ok_or_else(broken)?;
        let end = start
            .checked_add(usize::try_from(u32::from_be_bytes(length)).map_err(|_| broken())?)
            .ok_or_else(broken)?;
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(coded.get(start..end).ok_or_else(broken)?);
        at = end;
    }
    Ok(out)
}

/// An NV12 picture as P010 (M12.44): each 8-bit sample carried at 10 bits (shifted up by 2, as the CPU's HEVC
/// takes them), in the top 10 bits of a little-endian 16-bit word.
fn p010_of(nv12: &[u8]) -> Vec<u8> {
    nv12.iter().flat_map(|sample| [0, *sample]).collect()
}

/// The NAL units of an Annex B access unit (each without its start code).
fn annex_b_units(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut at = 0usize;
    while let Some(window) = data.get(at..at.saturating_add(3)) {
        if window == [0, 0, 1] {
            starts.push(at.saturating_add(3));
            at = at.saturating_add(3);
        } else {
            at = at.saturating_add(1);
        }
    }
    starts
        .iter()
        .enumerate()
        .filter_map(|(place, start)| {
            let mut end = starts
                .get(place.saturating_add(1))
                .map_or(data.len(), |next| next.saturating_sub(3));
            // A four-byte start code leaves a zero byte before the next one.
            while end > *start && data.get(end.saturating_sub(1)) == Some(&0) {
                end = end.saturating_sub(1);
            }
            data.get(*start..end).filter(|unit| !unit.is_empty())
        })
        .collect()
}

/// A hardware encoder's HEVC access unit as the Blu-ray's (M12.44, as [`hevc_annex_b`]): an access unit delimiter
/// first, then on an IDR picture (`key`) the parameter sets (VPS, SPS, PPS: the picture's own, or else the last ones
/// `sets` keeps), then the rest of its NAL units in their order, each after a four-byte start code.
fn hevc_hardware_unit(
    data: &[u8],
    key: bool,
    sets: &mut Vec<u8>,
) -> Result<Vec<u8>, TranscodeError> {
    let units = annex_b_units(data);
    if units.is_empty() {
        return Err(TranscodeError::Encode(
            "an empty HEVC access unit from the hardware encoder".to_string(),
        ));
    }
    let kind = |unit: &[u8]| unit.first().map_or(0, |byte| (byte >> 1) & 0x3F);
    let mut own = Vec::new();
    for unit in units.iter().filter(|unit| (32..=34).contains(&kind(unit))) {
        own.extend_from_slice(&[0, 0, 0, 1]);
        own.extend_from_slice(unit);
    }
    if !own.is_empty() {
        sets.clone_from(&own);
    }
    if key && sets.is_empty() {
        return Err(TranscodeError::Encode(
            "an HEVC IDR picture without parameter sets from the hardware encoder".to_string(),
        ));
    }
    let mut out = Vec::with_capacity(data.len().saturating_add(sets.len()).saturating_add(8));
    // nal_unit_type 35 (AUD), layer 0, temporal id 1; pic_type 2 and the RBSP stop bit.
    out.extend_from_slice(&[0, 0, 0, 1, 0x46, 0x01, 0x50]);
    if key || !own.is_empty() {
        out.extend_from_slice(sets);
    }
    for unit in units.iter().filter(|unit| !(32..=35).contains(&kind(unit))) {
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(unit);
    }
    Ok(out)
}

/// One access unit of the elementary stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessUnit {
    /// Byte offset of the access unit in the elementary stream.
    pub offset: u64,
    /// Byte length of the access unit.
    pub len: u32,
    /// Presentation time in 90 kHz units.
    pub pts: i64,
    /// Decode time in 90 kHz units.
    pub dts: i64,
    /// Whether the access unit starts with a keyframe.
    pub keyframe: bool,
}

/// Magic bytes for the access-unit index file.
const MAGIC: &[u8; 4] = b"DCH1";
/// Size of the index header in bytes.
const INDEX_HEADER_SIZE: usize = 8;
/// Size of one index entry in bytes.
const INDEX_ENTRY_SIZE: usize = 29;
/// Fixed-point shift used for bilinear sampling.
const FIXED_POINT_SHIFT: u32 = 16;
/// Fixed-point representation of one.
const FIXED_POINT_ONE: u64 = 1 << FIXED_POINT_SHIFT;
/// Fixed-point representation of one as u32.
const FIXED_POINT_ONE_U32: u32 = 1 << FIXED_POINT_SHIFT;
/// Maximum supported output dimension.
const MAX_DIMENSION: u32 = 4096;
/// Progress reporting interval in frames.
const PROGRESS_INTERVAL: u64 = 10;

/// Rectangle where source content is placed in the HD frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Geometry {
    /// Left coordinate.
    x: u32,
    /// Top coordinate.
    y: u32,
    /// Content width.
    width: u32,
    /// Content height.
    height: u32,
}

/// Owned YUV 4:2:0 planes for one frame.
struct YuvPlanes {
    /// Luma plane.
    y: Vec<u8>,
    /// Chroma blue plane.
    u: Vec<u8>,
    /// Chroma red plane.
    v: Vec<u8>,
    /// Luma stride.
    y_stride: usize,
    /// Chroma stride.
    uv_stride: usize,
}

impl YuvPlanes {
    /// Borrow the planes as an encoder frame.
    fn frame(&self) -> YuvFrame<'_> {
        YuvFrame {
            y: &self.y,
            u: &self.u,
            v: &self.v,
            y_stride: self.y_stride,
            uv_stride: self.uv_stride,
        }
    }
}

/// Writes elementary-stream packets and collects their index entries.
struct PacketWriter<'a, W: Write> {
    /// Underlying byte writer.
    writer: &'a mut W,
    /// Path used for I/O error reporting.
    path: &'a Path,
    /// Current stream offset.
    offset: u64,
    /// Collected access-unit entries.
    units: Vec<AccessUnit>,
}

impl<'a, W: Write> PacketWriter<'a, W> {
    /// Create a packet writer.
    fn new(writer: &'a mut W, path: &'a Path) -> Self {
        Self {
            writer,
            path,
            offset: 0,
            units: Vec::new(),
        }
    }

    /// Write one packet and append its index entry.
    fn write_packet(&mut self, packet: &Coded, options: &HdOptions) -> Result<(), TranscodeError> {
        let len = u32::try_from(packet.data.len())
            .map_err(|_| TranscodeError::Unsupported("packet too large".to_string()))?;
        self.writer
            .write_all(&packet.data)
            .map_err(|error| io_error(self.path, error))?;

        let start = self.offset;
        self.offset = start
            .checked_add(u64::from(len))
            .ok_or_else(|| TranscodeError::Unsupported("stream offset overflow".to_string()))?;

        let pts = frame_time_to_90k(packet.pts, options)?;
        let dts = frame_time_to_90k(packet.dts, options)?;
        self.units.push(AccessUnit {
            offset: start,
            len,
            pts,
            dts,
            keyframe: packet.keyframe,
        });
        Ok(())
    }

    /// Flush the underlying writer.
    fn flush(&mut self) -> Result<(), TranscodeError> {
        self.writer
            .flush()
            .map_err(|error| io_error(self.path, error))
    }

    /// Borrow collected index entries.
    fn units(&self) -> &[AccessUnit] {
        &self.units
    }
}

/// Encode the video of `input` to an Annex B H.264 stream and access-unit index.
pub fn transcode_h264(
    input: &Path,
    es_out: &Path,
    index_out: &Path,
    options: &HdOptions,
    progress: &mut dyn FnMut(f32),
) -> Result<u64, TranscodeError> {
    transcode_h264_range(input, es_out, index_out, options, (Tick(0), None), progress)
}

/// The part of a source of `total` length that `range` (a start and an end, `None` = the source's end) names:
/// where it starts and how long it is. A range outside the source is cut to it.
fn source_range(total: Tick, range: (Tick, Option<Tick>)) -> (Tick, Tick) {
    let total = total.0.max(0);
    let start = range.0.0.clamp(0, total);
    let end = range.1.map_or(total, |end| end.0.clamp(start, total));
    (Tick(start), Tick(end.saturating_sub(start)))
}

/// [`transcode_h264`] of a part of the input: from `range.0` to `range.1` of its time (`None`: to its end). The
/// stream's times start at zero.
pub fn transcode_h264_range(
    input: &Path,
    es_out: &Path,
    index_out: &Path,
    options: &HdOptions,
    range: (Tick, Option<Tick>),
    progress: &mut dyn FnMut(f32),
) -> Result<u64, TranscodeError> {
    validate_options(options)?;
    let source = Source::open(input)?;
    let (range_start, range_length) = source_range(source.info().duration, range);
    let frames = frame_count(range_length, options)?;
    let kept = kept_fields(&source, options);
    let fields = kept.is_some();

    // M12.40: the registered hardware encoder first, for the streams it makes (H.264 in one pass; HEVC, M12.44, is
    // always one); a failure (but a stop) encodes the video again on the CPU.
    if (options.mode != crate::RateMode::Vbr2Pass || options.codec == HdCodec::Hevc)
        && let Some(mut encoder) = HdEncoder::hardware(options, &HdPass::Single)
    {
        let made = (|| -> Result<(), TranscodeError> {
            let file = File::create(es_out).map_err(|error| io_error(es_out, error))?;
            let mut writer = BufWriter::new(file);
            let mut packets = PacketWriter::new(&mut writer, es_out);
            encode_range(
                (&source, range_start, frames),
                (options, fields),
                &mut encoder,
                &mut packets,
                progress,
            )?;
            packets.flush()?;
            write_h264_index(index_out, packets.units())
        })();
        match made {
            Ok(()) => {
                progress(1.0);
                return Ok(frames);
            }
            Err(error) if crate::stop_requested() => return Err(error),
            Err(_) => {}
        }
    }

    // VBR 2 pass (M6.328): the video is gone through once to measure it, writing nothing, then encoded by what
    // was measured; the progress runs once over both passes. HEVC is encoded in one (M12.43).
    let two_pass =
        options.mode == crate::RateMode::Vbr2Pass && frames > 1 && options.codec != HdCodec::Hevc;
    let pass = if two_pass {
        let mut first = HdEncoder::with_pass(options, HdPass::First, kept)?;
        let mut nowhere = std::io::sink();
        let mut discarded = PacketWriter::new(&mut nowhere, es_out);
        encode_range(
            (&source, range_start, frames),
            (options, fields),
            &mut first,
            &mut discarded,
            &mut |fraction| progress(fraction * 0.5),
        )?;
        let stats = first
            .stats()
            .ok_or_else(|| TranscodeError::Encode("the first pass measured nothing".to_string()))?;
        HdPass::Second(stats)
    } else {
        HdPass::Single
    };
    let (from, span) = if two_pass { (0.5, 0.5) } else { (0.0, 1.0) };

    let file = File::create(es_out).map_err(|error| io_error(es_out, error))?;
    let mut writer = BufWriter::new(file);
    let mut encoder = HdEncoder::with_pass(options, pass, kept)?;
    let mut packets = PacketWriter::new(&mut writer, es_out);
    encode_range(
        (&source, range_start, frames),
        (options, fields),
        &mut encoder,
        &mut packets,
        &mut |fraction| progress(from + fraction * span),
    )?;
    packets.flush()?;
    write_h264_index(index_out, packets.units())?;
    progress(1.0);
    Ok(frames)
}

/// Whether an HD stream keeps the two fields of an interlaced source apart (M12.37, as a DVD's since M12.26), and
/// then whether the top field comes first: the output is interlaced (MPEG-2 at 29.97 or 25 frames a second, or H.264
/// coded as interlaced frames) and the source is interlaced at the output's frame rate. M12.42: a source whose bottom
/// field comes first (DV, some camcorders) too, the stream then marked so (each field is scaled from the lines of
/// its own parity, whichever comes first); `None`: the frames are scaled whole.
fn kept_fields(source: &Source, options: &HdOptions) -> Option<bool> {
    let interlaced = match options.codec {
        HdCodec::Mpeg2 => matches!(mpeg2_config(options), Ok((_, None))),
        HdCodec::H264 => options.interlaced,
        HdCodec::Hevc => false,
    };
    let rate = source.info().video.as_ref().map(|video| video.frame_rate);
    let same_rate = rate.is_some_and(|rate| {
        rate.den != 0
            && i128::from(rate.num) * i128::from(options.fps_den)
                == i128::from(options.fps_num) * i128::from(rate.den)
    });
    if !interlaced || !same_rate {
        return None;
    }
    match crate::field_type::field_type(source.path())? {
        crate::FieldType::UpperFirst => Some(true),
        crate::FieldType::LowerFirst => Some(false),
        crate::FieldType::Progressive => None,
    }
}

/// One pass over `frames` pictures of `source` from `range_start`: each decoded, scaled and converted on a thread of
/// its own, a few ahead of `encoder`, whose packets go to `packets` (the encoder flushed at the end). `fields`: each
/// field of an interlaced source scaled and its colour subsampled on its own ([`kept_fields`]).
fn encode_range<W: Write>(
    (source, range_start, frames): (&Source, Tick, u64),
    (options, fields): (&HdOptions, bool),
    encoder: &mut HdEncoder,
    packets: &mut PacketWriter<'_, W>,
    progress: &mut dyn FnMut(f32),
) -> Result<(), TranscodeError> {
    // The pictures are decoded, scaled and converted on a thread of their own, a few ahead of the encoder, so the
    // two overlap.
    let (sender, receiver) =
        std::sync::mpsc::sync_channel::<Result<YuvPlanes, TranscodeError>>(PIPELINE_DEPTH);
    std::thread::scope(|scope| -> Result<(), TranscodeError> {
        let _ = scope.spawn(move || {
            for frame_index in 0..frames {
                let planes = (|| {
                    let time = frame_tick(frame_index, options)?;
                    let time = Tick(time.0.saturating_add(range_start.0));
                    let rgb = crate::blend::picture_at(
                        source,
                        time,
                        options.rendering.frame_blend,
                        &|frame| {
                            if fields {
                                render_rgb_fields_frame(frame, options)
                            } else {
                                render_rgb_frame(frame, options)
                            }
                        },
                    )?;
                    rgb_to_yuv_as(&rgb, options, fields)
                })();
                let failed = planes.is_err();
                // A closed channel: the encoder stopped (an error); nothing more to do.
                if sender.send(planes).is_err() || failed {
                    break;
                }
            }
        });
        // Owned by this closure, so an early return (an encode or write error, a stop) drops it: the decoding thread
        // blocked on a full channel then sees it closed and stops, instead of `scope` waiting for it forever (a
        // hang seen in the HD tests, M6.348).
        let receiver = receiver;
        for frame_index in 0..frames {
            let planes = receiver
                .recv()
                .map_err(|_| TranscodeError::Encode("the decoder stopped early".to_string()))??;
            encode_planes(encoder, &planes, frame_index, options, packets)?;
            report_progress(progress, frame_index, frames);
            crate::check_stop()?;
        }
        Ok(())
    })?;

    finish_encoder(encoder, options, packets)
}

/// The most black pictures one stream holds (three hours at 60 frames a second).
const MAX_BLACK_FRAMES: u64 = 60 * 60 * 60 * 3;

/// `frames` black pictures of `options`' size and rate as an H.264 stream with its index, as [`transcode_h264`]
/// writes them (what plays in a gap between video clips).
///
/// # Errors
///
/// When there are no or too many frames, or the stream cannot be encoded or written.
pub fn encode_h264_black(
    frames: u64,
    es_out: &Path,
    index_out: &Path,
    options: &HdOptions,
) -> Result<u64, TranscodeError> {
    validate_options(options)?;
    if frames == 0 || frames > MAX_BLACK_FRAMES {
        return Err(TranscodeError::Unsupported(
            "a gap must be between one frame and three hours".to_string(),
        ));
    }
    let pixels = usize::try_from(options.width)
        .ok()
        .and_then(|width| width.checked_mul(usize::try_from(options.height).ok()?))
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or_else(|| TranscodeError::Unsupported("picture too large".to_string()))?;
    let planes = rgb_to_yuv(&vec![0_u8; pixels], options)?;
    let file = File::create(es_out).map_err(|error| io_error(es_out, error))?;
    let mut writer = BufWriter::new(file);
    let mut encoder = HdEncoder::new(options)?;
    let mut packets = PacketWriter::new(&mut writer, es_out);
    for frame_index in 0..frames {
        encode_planes(&mut encoder, &planes, frame_index, options, &mut packets)?;
    }
    finish_encoder(&mut encoder, options, &mut packets)?;
    packets.flush()?;
    write_h264_index(index_out, packets.units())?;
    Ok(frames)
}

/// One RGB24 picture of `options`' size as an H.264 stream of a single IDR picture (an HD menu's still), at a
/// constant quantiser fine enough for text. With [`HdCodec::Mpeg2`], an MPEG-2 sequence of one I picture, ended by
/// its sequence end code.
pub fn encode_h264_still(rgb: &[u8], options: &HdOptions) -> Result<Vec<u8>, TranscodeError> {
    validate_options(options)?;
    let expected = usize::try_from(options.width)
        .ok()
        .and_then(|width| width.checked_mul(usize::try_from(options.height).ok()?))
        .and_then(|pixels| pixels.checked_mul(3));
    if expected != Some(rgb.len()) {
        return Err(TranscodeError::Unsupported(
            "still picture size mismatch".to_string(),
        ));
    }
    if options.codec == HdCodec::Mpeg2 {
        let (config, _) = mpeg2_config(options)?;
        let frame = mpeg2_frame(
            &rgb_to_yuv(rgb, options)?,
            config.format.coded_size(config.standard),
        )?;
        return mpeg2_encoder(options)?
            .encode_still(&frame, MPEG2_STILL_CODE)
            .map_err(|error| TranscodeError::Encode(error.to_string()));
    }
    let mut config = encoder_config(options);
    config.rate = RateControl::Qp(STILL_QP);
    config.intra_only = true;
    config.bframes = 0;
    let mut encoder =
        Encoder::new(config).map_err(|error| TranscodeError::Encode(error.to_string()))?;
    let planes = rgb_to_yuv(rgb, options)?;
    let mut data = Vec::new();
    for packet in encoder
        .encode(&planes.frame(), 0)
        .map_err(|error| TranscodeError::Encode(error.to_string()))?
    {
        data.extend_from_slice(&packet.data);
    }
    for packet in encoder.flush() {
        data.extend_from_slice(&packet.data);
    }
    if data.is_empty() {
        return Err(TranscodeError::Encode("no picture was encoded".to_string()));
    }
    Ok(data)
}

/// An H.264 encoder fed RGB24 pictures one by one (an HD motion menu): the Annex B stream and its access units
/// (time stamps in 90 kHz from 0).
pub struct H264Frames {
    encoder: HdEncoder,
    options: HdOptions,
    data: Vec<u8>,
    units: Vec<AccessUnit>,
    next: i64,
}

impl H264Frames {
    /// A new encoder for pictures of `options`' size and rate.
    ///
    /// # Errors
    ///
    /// When the options are out of range or the encoder cannot start.
    pub fn new(options: &HdOptions) -> Result<Self, TranscodeError> {
        validate_options(options)?;
        let encoder = HdEncoder::new(options)?;
        Ok(Self {
            encoder,
            options: options.clone(),
            data: Vec::new(),
            units: Vec::new(),
            next: 0,
        })
    }

    /// Encodes the next picture (RGB24 of the encoder's size).
    ///
    /// # Errors
    ///
    /// When the picture has the wrong size or cannot be encoded.
    pub fn push_rgb(&mut self, rgb: &[u8]) -> Result<(), TranscodeError> {
        let expected = usize::try_from(self.options.width)
            .ok()
            .and_then(|width| width.checked_mul(usize::try_from(self.options.height).ok()?))
            .and_then(|pixels| pixels.checked_mul(3));
        if expected != Some(rgb.len()) {
            return Err(TranscodeError::Unsupported(
                "picture size mismatch".to_string(),
            ));
        }
        let planes = rgb_to_yuv(rgb, &self.options)?;
        let packets = self.encoder.encode(&planes, self.next, &self.options)?;
        self.next = self.next.saturating_add(1);
        for packet in &packets {
            self.add(packet)?;
        }
        Ok(())
    }

    /// Flushes the encoder: the stream and its access units.
    ///
    /// # Errors
    ///
    /// When a time stamp is out of range.
    pub fn finish(mut self) -> Result<(Vec<u8>, Vec<AccessUnit>), TranscodeError> {
        for packet in &self.encoder.flush()? {
            self.add(packet)?;
        }
        Ok((self.data, self.units))
    }

    fn add(&mut self, packet: &Coded) -> Result<(), TranscodeError> {
        let offset = u64::try_from(self.data.len())
            .map_err(|_| TranscodeError::Unsupported("stream too large".to_string()))?;
        let len = u32::try_from(packet.data.len())
            .map_err(|_| TranscodeError::Unsupported("packet too large".to_string()))?;
        self.data.extend_from_slice(&packet.data);
        self.units.push(AccessUnit {
            offset,
            len,
            pts: frame_time_to_90k(packet.pts, &self.options)?,
            dts: frame_time_to_90k(packet.dts, &self.options)?,
            keyframe: packet.keyframe,
        });
        Ok(())
    }
}

/// Calls `f` with each of the first `frames` pictures of the video `input` at `options`' rate, as RGB24 of
/// `options`' size (fitted, with bars), for an HD motion menu's video background.
///
/// # Errors
///
/// When the video cannot be read, or `f` fails.
pub fn for_each_hd_frame(
    input: &Path,
    options: &HdOptions,
    frames: u64,
    f: &mut dyn FnMut(u64, &[u8]) -> Result<(), String>,
) -> Result<(), TranscodeError> {
    validate_options(options)?;
    let source = Source::open(input)?;
    for index in 0..frames {
        let time = frame_tick(index, options)?;
        let frame = source.video_frame(time)?;
        let rgb = render_rgb_frame(&frame, options)?;
        f(index, &rgb).map_err(TranscodeError::Encode)?;
    }
    Ok(())
}

/// Quantiser of an HD still (low: menus have text).
const STILL_QP: u8 = 18;
/// quantiser_scale_code an MPEG-2 HD still starts at (the finest that fits the VBV buffer is used).
const MPEG2_STILL_CODE: u8 = 2;

/// Read an index written by `transcode_h264`.
pub fn read_h264_index(path: &Path) -> Result<Vec<AccessUnit>, TranscodeError> {
    let metadata = fs::metadata(path).map_err(|error| io_error(path, error))?;
    let file_size = usize::try_from(metadata.len())
        .map_err(|_| TranscodeError::Unsupported("index file too large".to_string()))?;
    if file_size < INDEX_HEADER_SIZE {
        return Err(TranscodeError::Unsupported(
            "index file is truncated".to_string(),
        ));
    }

    let bytes = fs::read(path).map_err(|error| io_error(path, error))?;
    if !bytes.starts_with(MAGIC) {
        return Err(TranscodeError::Unsupported("bad index magic".to_string()));
    }

    let count = read_u32_at(&bytes, 4)?;
    let count_usize = usize::try_from(count)
        .map_err(|_| TranscodeError::Unsupported("index count too large".to_string()))?;
    let expected = INDEX_HEADER_SIZE
        .checked_add(
            count_usize
                .checked_mul(INDEX_ENTRY_SIZE)
                .ok_or_else(|| TranscodeError::Unsupported("index size overflow".to_string()))?,
        )
        .ok_or_else(|| TranscodeError::Unsupported("index size overflow".to_string()))?;
    if bytes.len() != expected {
        return Err(TranscodeError::Unsupported(
            "index size mismatch".to_string(),
        ));
    }

    let mut units = Vec::with_capacity(count_usize);
    let entries = bytes
        .get(INDEX_HEADER_SIZE..)
        .ok_or_else(|| TranscodeError::Unsupported("index entries missing".to_string()))?;
    for entry in entries.as_chunks::<INDEX_ENTRY_SIZE>().0 {
        units.push(parse_entry(entry)?);
    }
    if units.len() != count_usize {
        return Err(TranscodeError::Unsupported(
            "index entry count mismatch".to_string(),
        ));
    }
    Ok(units)
}

/// Validate HD options.
fn validate_options(options: &HdOptions) -> Result<(), TranscodeError> {
    if options.width == 0
        || options.height == 0
        || !options.width.is_multiple_of(2)
        || !options.height.is_multiple_of(2)
        || options.width > MAX_DIMENSION
        || options.height > MAX_DIMENSION
    {
        return Err(TranscodeError::Unsupported(
            "width and height must be even and <= 4096".to_string(),
        ));
    }
    if options.fps_num == 0 || options.fps_den == 0 {
        return Err(TranscodeError::Unsupported(
            "frame rate must be positive".to_string(),
        ));
    }
    if options.target_kbps == 0 || options.max_kbps == 0 || options.target_kbps > options.max_kbps {
        return Err(TranscodeError::Unsupported(
            "bitrate must be positive and target <= max".to_string(),
        ));
    }
    Ok(())
}

/// Compute output frame count from source duration.
fn frame_count(duration: Tick, options: &HdOptions) -> Result<u64, TranscodeError> {
    let ticks = u128::try_from(duration.0)
        .map_err(|_| TranscodeError::Unsupported("duration is negative".to_string()))?;
    let ticks_per_second = u128::try_from(TICKS_PER_SECOND)
        .map_err(|_| TranscodeError::Unsupported("tick rate is invalid".to_string()))?;
    let numerator = ticks
        .checked_mul(u128::from(options.fps_num))
        .ok_or_else(|| TranscodeError::Unsupported("frame count overflow".to_string()))?;
    let denominator = ticks_per_second
        .checked_mul(u128::from(options.fps_den))
        .ok_or_else(|| TranscodeError::Unsupported("frame count overflow".to_string()))?;
    if denominator == 0 {
        return Err(TranscodeError::Unsupported(
            "frame count denominator is zero".to_string(),
        ));
    }
    // The nearest frame: a 6.0 s clip at 23.976 is 144 frames (143.86), all of the source.
    let count = if numerator == 0 {
        1
    } else {
        numerator.saturating_add(denominator / 2) / denominator
    };
    u64::try_from(count)
        .map_err(|_| TranscodeError::Unsupported("frame count too large".to_string()))
}

/// Compute the source tick for output frame `index`.
fn frame_tick(index: u64, options: &HdOptions) -> Result<Tick, TranscodeError> {
    let ticks_per_second = u128::try_from(TICKS_PER_SECOND)
        .map_err(|_| TranscodeError::Unsupported("tick rate is invalid".to_string()))?;
    let numerator = u128::from(index)
        .checked_mul(ticks_per_second)
        .and_then(|value| value.checked_mul(u128::from(options.fps_den)))
        .ok_or_else(|| TranscodeError::Unsupported("frame time overflow".to_string()))?;
    let denominator = u128::from(options.fps_num);
    if denominator == 0 {
        return Err(TranscodeError::Unsupported(
            "frame rate is zero".to_string(),
        ));
    }
    let ticks = numerator / denominator;
    let ticks_i64 = i64::try_from(ticks)
        .map_err(|_| TranscodeError::Unsupported("frame time too large".to_string()))?;
    Ok(Tick(ticks_i64))
}

/// Convert a frame-count time to 90 kHz units, rounding down.
fn frame_time_to_90k(frames: i64, options: &HdOptions) -> Result<i64, TranscodeError> {
    let numerator = i128::from(frames)
        .checked_mul(90_000)
        .and_then(|value| value.checked_mul(i128::from(options.fps_den)))
        .ok_or_else(|| TranscodeError::Unsupported("90 kHz time overflow".to_string()))?;
    let denominator = i128::from(options.fps_num);
    if denominator == 0 {
        return Err(TranscodeError::Unsupported(
            "frame rate is zero".to_string(),
        ));
    }
    let value = numerator.div_euclid(denominator);
    i64::try_from(value)
        .map_err(|_| TranscodeError::Unsupported("90 kHz time out of range".to_string()))
}

/// Build encoder configuration from HD options.
fn encoder_config(options: &HdOptions) -> EncoderConfig {
    let mut config = EncoderConfig::new(
        options.width,
        options.height,
        options.fps_num,
        options.fps_den,
    );
    config.profile = Profile::High;
    config.preset = Preset::Balanced;
    config.rate = if options.mode == crate::RateMode::Cbr {
        // CBR: the target is also the peak.
        RateControl::Cbr {
            kbps: options.target_kbps,
        }
    } else {
        RateControl::Vbr {
            target_kbps: options.target_kbps,
            max_kbps: options.max_kbps,
        }
    };
    config.keyint = keyint(options);
    config.bframes = 2;
    config.slices = 4;
    config.aud = true;
    config.level = Some(41);
    config.format = PacketFormat::AnnexB;
    if options.interlaced {
        config.interlaced = Some(craft_video::h264::FieldOrder::TopFirst);
    }
    // 1440 x 1080 is shown 16:9: its pixels are 4:3 wide (M10.40; the VUI's sample aspect ratio).
    let (wide, tall) = output_pixel_aspect(options.width, options.height, options.wide);
    config.sar = (
        u16::try_from(wide).unwrap_or(1),
        u16::try_from(tall).unwrap_or(1),
    );
    config
}

/// Compute one-second GOP length rounded to nearest frame.
fn keyint(options: &HdOptions) -> u32 {
    let num = u128::from(options.fps_num);
    let den = u128::from(options.fps_den);
    let rounded = num
        .checked_add(den / 2)
        .unwrap_or(num)
        .checked_div(den)
        .unwrap_or(1);
    let value = if rounded == 0 { 1 } else { rounded };
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Encode one output frame.
/// How many converted pictures the decoding thread keeps ready ahead of the encoder.
const PIPELINE_DEPTH: usize = 8;

/// Encodes one converted picture.
fn encode_planes<W: Write>(
    encoder: &mut HdEncoder,
    planes: &YuvPlanes,
    frame_index: u64,
    options: &HdOptions,
    packets: &mut PacketWriter<'_, W>,
) -> Result<(), TranscodeError> {
    let pts = i64::try_from(frame_index)
        .map_err(|_| TranscodeError::Unsupported("frame index too large".to_string()))?;
    let encoded = encoder.encode(planes, pts, options)?;
    for packet in &encoded {
        packets.write_packet(packet, options)?;
    }
    Ok(())
}

/// Flush the encoder and write remaining packets.
fn finish_encoder<W: Write>(
    encoder: &mut HdEncoder,
    options: &HdOptions,
    packets: &mut PacketWriter<'_, W>,
) -> Result<(), TranscodeError> {
    let rest = encoder.flush()?;
    for packet in &rest {
        packets.write_packet(packet, options)?;
    }
    Ok(())
}

/// `frame` as RGB24 at `width` x `height` with square pixels, the picture fitted in with bars: the HD transcode's
/// rendering, for showing a frame at the disc's HD size.
pub(crate) fn render_fit(
    frame: &VideoFrame,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, TranscodeError> {
    render_rgb_frame(
        frame,
        &HdOptions {
            width,
            height,
            fps_num: 1,
            fps_den: 1,
            target_kbps: 0,
            max_kbps: 0,
            codec: HdCodec::H264,
            quality: crate::BEST_QUALITY,
            rendering: crate::Rendering::default(),
            mode: crate::RateMode::Vbr,
            interlaced: false,
            wide: false,
        },
    )
}

/// Render a source frame into an RGB24 HD frame.
fn render_rgb_frame(frame: &VideoFrame, options: &HdOptions) -> Result<Vec<u8>, TranscodeError> {
    let geometry = target_geometry(
        frame.width,
        frame.height,
        frame.par,
        options.width,
        (options.height, options.wide),
    )
    .ok_or_else(|| TranscodeError::Unsupported("invalid frame geometry".to_string()))?;
    let rgba = frame.to_rgba8();
    // Encore's "Use Maximum Render Quality": the filtered scaler.
    let rendered = if options.rendering.max_quality {
        crate::resample::render_rgb_filtered(
            &rgba,
            (frame.width, frame.height),
            (geometry.x, geometry.y, geometry.width, geometry.height),
            (options.width, options.height),
        )
    } else {
        render_rgb(
            &rgba,
            frame.width,
            frame.height,
            geometry,
            options.width,
            options.height,
        )
    };
    rendered.ok_or_else(|| TranscodeError::Unsupported("failed to render frame".to_string()))
}

/// [`render_rgb_frame`] for an interlaced source whose fields stay apart ([`kept_fields`]): each output line made of
/// the source lines of its own field ([`render_rgb_fields_at`]). "Use Maximum Render Quality" does not change it,
/// as on a DVD.
fn render_rgb_fields_frame(
    frame: &VideoFrame,
    options: &HdOptions,
) -> Result<Vec<u8>, TranscodeError> {
    let geometry = target_geometry(
        frame.width,
        frame.height,
        frame.par,
        options.width,
        (options.height, options.wide),
    )
    .ok_or_else(|| TranscodeError::Unsupported("invalid frame geometry".to_string()))?;
    render_rgb_rows(
        &frame.to_rgba8(),
        (frame.width, frame.height),
        geometry,
        (options.width, options.height),
        true,
    )
    .ok_or_else(|| TranscodeError::Unsupported("failed to render frame".to_string()))
}

/// The shape of an output frame's pixels: HDV's 1440 x 1080 is shown 16:9, its pixels 4:3 wide (M6.393); a
/// standard-definition frame (M10.41) is shown 4:3 or, `wide`, 16:9 across its whole width, as a DVD's (8:9 and
/// 32:27 for 720 x 480, 16:15 and 64:45 for 720 x 576); the other HD sizes have square pixels.
fn output_pixel_aspect(width: u32, height: u32, wide: bool) -> (u32, u32) {
    match (width, height, wide) {
        (1440, 1080, _) => (4, 3),
        (720, 480, false) => (8, 9),
        (720, 480, true) => (32, 27),
        (720, 576, false) => (16, 15),
        (720, 576, true) => (64, 45),
        _ => (1, 1),
    }
}

/// Compute letterbox/pillarbox geometry for an HD output frame (of pixels shaped as [`output_pixel_aspect`] says).
fn target_geometry(
    source_width: u32,
    source_height: u32,
    par: (u32, u32),
    output_width: u32,
    (output_height, output_wide): (u32, bool),
) -> Option<Geometry> {
    let (wide, tall) = output_pixel_aspect(output_width, output_height, output_wide);
    if (wide, tall) == (1, 1) {
        return square_geometry(
            source_width,
            source_height,
            par,
            output_width,
            output_height,
        );
    }
    // Fitted into the frame as it is shown, then back to its pixels.
    let shown = square_geometry(
        source_width,
        source_height,
        par,
        output_width.checked_mul(wide)?,
        output_height.checked_mul(tall)?,
    )?;
    let width = (shown.width.saturating_add(wide / 2) / wide).clamp(1, output_width);
    let height = (shown.height.saturating_add(tall / 2) / tall).clamp(1, output_height);
    Some(Geometry {
        x: output_width.saturating_sub(width) / 2,
        y: output_height.saturating_sub(height) / 2,
        width,
        height,
    })
}

/// [`target_geometry`] for an output frame of square pixels.
fn square_geometry(
    source_width: u32,
    source_height: u32,
    par: (u32, u32),
    output_width: u32,
    output_height: u32,
) -> Option<Geometry> {
    if source_width == 0 || source_height == 0 || output_width == 0 || output_height == 0 {
        return None;
    }

    let par_num = if par.0 == 0 { 1 } else { par.0 };
    let par_den = if par.1 == 0 { 1 } else { par.1 };
    let aspect_num = u128::from(source_width).checked_mul(u128::from(par_num))?;
    let aspect_den = u128::from(source_height).checked_mul(u128::from(par_den))?;
    if aspect_num == 0 || aspect_den == 0 {
        return None;
    }

    let left = aspect_num.checked_mul(u128::from(output_height))?;
    let right = u128::from(output_width).checked_mul(aspect_den)?;
    let (width, height) = if left == right {
        (output_width, output_height)
    } else if left > right {
        let height = scaled_height(output_width, output_height, aspect_num, aspect_den)?;
        (output_width, height)
    } else {
        let width = scaled_width(output_width, output_height, aspect_num, aspect_den)?;
        (width, output_height)
    };

    let width = width.clamp(1, output_width);
    let height = height.clamp(1, output_height);
    let x = output_width.saturating_sub(width) / 2;
    let y = output_height.saturating_sub(height) / 2;
    Some(Geometry {
        x,
        y,
        width,
        height,
    })
}

/// Scale height when source aspect is wider than output.
fn scaled_height(
    output_width: u32,
    output_height: u32,
    aspect_num: u128,
    aspect_den: u128,
) -> Option<u32> {
    if aspect_num == 0 {
        return Some(output_height);
    }
    let numerator = u128::from(output_width).checked_mul(aspect_den)?;
    Some(clamp_u128(numerator / aspect_num, output_height))
}

/// Scale width when source aspect is taller than output.
fn scaled_width(
    output_width: u32,
    output_height: u32,
    aspect_num: u128,
    aspect_den: u128,
) -> Option<u32> {
    if aspect_den == 0 {
        return Some(output_width);
    }
    let numerator = u128::from(output_height).checked_mul(aspect_num)?;
    Some(clamp_u128(numerator / aspect_den, output_width))
}

/// Clamp a u128 value to a u32 maximum.
fn clamp_u128(value: u128, max: u32) -> u32 {
    if value > u128::from(max) {
        max
    } else {
        u32::try_from(value).unwrap_or(max)
    }
}

/// [`render_rgb`] for the other transcodes (the DVD's 720 pixel wide frame): the picture at `(x, y)` of the
/// output, `(width, height)` large.
pub(crate) fn render_rgb_at(
    rgba: &[u8],
    source: (u32, u32),
    (x, y, width, height): (u32, u32, u32, u32),
    output: (u32, u32),
) -> Option<Vec<u8>> {
    let geometry = Geometry {
        x,
        y,
        width,
        height,
    };
    render_rgb(rgba, source.0, source.1, geometry, output.0, output.1)
}

/// [`render_rgb_at`] for an interlaced picture (M12.26): each output line is made of the source lines of its own
/// field (the even lines of the top field, the odd ones of the bottom field), at the place the line has in the
/// frame, so the two moments of the picture stay apart when it is scaled.
pub(crate) fn render_rgb_fields_at(
    rgba: &[u8],
    source: (u32, u32),
    (x, y, width, height): (u32, u32, u32, u32),
    output: (u32, u32),
) -> Option<Vec<u8>> {
    let geometry = Geometry {
        x,
        y,
        width,
        height,
    };
    render_rgb_rows(
        rgba,
        (source.0, source.1),
        geometry,
        (output.0, output.1),
        true,
    )
}

/// The two source lines (and the weight of the second, in [`FIXED_POINT_SHIFT`] bits) of output line `position` of an
/// interlaced picture: its place in the frame ([`source_coordinate`]), taken to the lines of its field (the
/// output line's parity), the second line of the field clamped at the field's last.
fn field_rows(
    position: u32,
    (origin, content): (u32, u32),
    source: u32,
    lines: usize,
) -> (usize, usize, u32) {
    let parity = usize::try_from(position % 2).unwrap_or(0);
    let frame = source_coordinate(position, origin, content, source);
    // The field's own coordinate: (frame - parity) / 2 lines.
    let field = frame.saturating_sub(u64::try_from(parity).unwrap_or(0) * FIXED_POINT_ONE) / 2;
    let last_line = lines.saturating_sub(1);
    // The field's last line: the last line of that parity (the other field's when the picture has one line).
    let field_last = if last_line >= parity {
        (last_line - parity) / 2
    } else {
        0
    };
    let line = |index: usize| (index * 2 + parity).min(last_line);
    let first = usize::try_from(field >> FIXED_POINT_SHIFT)
        .unwrap_or(field_last)
        .min(field_last);
    let second = first.saturating_add(1).min(field_last);
    let weight = u32::try_from(field & (FIXED_POINT_ONE - 1)).unwrap_or(0);
    (line(first), line(second), weight)
}

/// Render RGBA source into RGB24 output with bilinear sampling, the picture in `geometry` and black around it.
///
/// The arithmetic is [`sample_bilinear`]'s, pixel for pixel: each source pixel composited onto black once, the
/// columns' and rows' source positions and weights worked out once, and the rows shared out between the CPU's
/// cores (a 1920 x 1080 frame for the HD transcode and the preview's HD titles).
fn render_rgb(
    rgba: &[u8],
    source_width: u32,
    source_height: u32,
    geometry: Geometry,
    output_width: u32,
    output_height: u32,
) -> Option<Vec<u8>> {
    render_rgb_rows(
        rgba,
        (source_width, source_height),
        geometry,
        (output_width, output_height),
        false,
    )
}

/// [`render_rgb`], with `fields` each output line made of its own field's lines ([`render_rgb_fields_at`]).
fn render_rgb_rows(
    rgba: &[u8],
    (source_width, source_height): (u32, u32),
    geometry: Geometry,
    (output_width, output_height): (u32, u32),
    fields: bool,
) -> Option<Vec<u8>> {
    let width = usize::try_from(output_width).ok()?;
    let height = usize::try_from(output_height).ok()?;
    let size = width.checked_mul(height)?.checked_mul(3)?;
    let mut rgb = vec![0u8; size];
    let end_x = geometry.x.checked_add(geometry.width)?;
    let end_y = geometry.y.checked_add(geometry.height)?;
    if geometry.width == 0 || geometry.height == 0 {
        return Some(rgb);
    }
    let sw = usize::try_from(source_width).ok()?;
    let sh = usize::try_from(source_height).ok()?;
    if sw == 0 || sh == 0 || end_x > output_width || end_y > output_height {
        return None;
    }
    let source_pixels = sw.checked_mul(sh)?;
    let (pixels, _) = rgba.as_chunks::<4>();
    let pixels = pixels.get(..source_pixels)?;

    let axis = |position: u32, origin: u32, content: u32, source: u32, last: usize| {
        let at = source_coordinate(position, origin, content, source);
        let first = usize::try_from(at >> FIXED_POINT_SHIFT)
            .unwrap_or(last)
            .min(last);
        let second = first.saturating_add(1).min(last);
        let weight = u32::try_from(at & (FIXED_POINT_ONE - 1)).unwrap_or(0);
        (first, second, weight)
    };
    let columns: Vec<(usize, usize, u32)> = (geometry.x..end_x)
        .map(|x| axis(x, geometry.x, geometry.width, source_width, sw - 1))
        .collect();
    let rows: Vec<(usize, usize, u32)> = (geometry.y..end_y)
        .map(|y| {
            if fields {
                field_rows(y, (geometry.y, geometry.height), source_height, sh)
            } else {
                axis(y, geometry.y, geometry.height, source_height, sh - 1)
            }
        })
        .collect();

    let stride = width.checked_mul(3)?;
    let left = usize::try_from(geometry.x).ok()?.checked_mul(3)?;
    let top = usize::try_from(geometry.y).ok()?;
    let content_rows = rgb.get_mut(top.checked_mul(stride)?..)?;
    let place = Place {
        stride,
        left,
        source_width: sw,
        columns: &columns,
        rows: &rows,
    };
    // A picture without see-through pixels (every video frame) is read as it is: composited onto black, an
    // opaque pixel keeps its colour, so the copy that compositing makes is not needed.
    if pixels.iter().all(|pixel| pixel[3] == 255) {
        fill_rows(content_rows, &place, pixels, |pixel: [u8; 4]| {
            [pixel[0], pixel[1], pixel[2]]
        });
    } else {
        // Composited onto black once.
        let composited: Vec<[u8; 3]> = pixels.iter().map(|pixel| composite_pixel(*pixel)).collect();
        fill_rows(content_rows, &place, &composited, |pixel: [u8; 3]| pixel);
    }
    Some(rgb)
}

/// [`render_rgb`] for a picture placed anywhere (a slideshow's pan and zoom, M6.365): at `(x, y)` of the output,
/// `(width, height)` large, cut where it reaches past the frame. The arithmetic is the same as for a picture inside
/// the frame: each column and row takes its source position from where it lies in the whole picture.
pub(crate) fn render_rgb_placed(
    rgba: &[u8],
    (source_width, source_height): (u32, u32),
    (x, y, width, height): (i64, i64, u32, u32),
    (output_width, output_height): (u32, u32),
) -> Option<Vec<u8>> {
    let out_w = usize::try_from(output_width).ok()?;
    let out_h = usize::try_from(output_height).ok()?;
    let mut rgb = vec![0u8; out_w.checked_mul(out_h)?.checked_mul(3)?];
    // The part of the picture inside the frame.
    let (x0, x1) = (
        x.max(0),
        x.checked_add(i64::from(width))?
            .min(i64::from(output_width)),
    );
    let (y0, y1) = (
        y.max(0),
        y.checked_add(i64::from(height))?
            .min(i64::from(output_height)),
    );
    if width == 0 || height == 0 || x0 >= x1 || y0 >= y1 {
        return Some(rgb);
    }
    let sw = usize::try_from(source_width).ok()?;
    let sh = usize::try_from(source_height).ok()?;
    if sw == 0 || sh == 0 {
        return None;
    }
    let (pixels, _) = rgba.as_chunks::<4>();
    let pixels = pixels.get(..sw.checked_mul(sh)?)?;
    let axis = |offset: i64, content: u32, source: u32, last: usize| {
        let at = source_coordinate(u32::try_from(offset).ok()?, 0, content, source);
        let first = usize::try_from(at >> FIXED_POINT_SHIFT)
            .unwrap_or(last)
            .min(last);
        let second = first.saturating_add(1).min(last);
        let weight = u32::try_from(at & (FIXED_POINT_ONE - 1)).unwrap_or(0);
        Some((first, second, weight))
    };
    let columns = (x0..x1)
        .map(|column| axis(column - x, width, source_width, sw - 1))
        .collect::<Option<Vec<_>>>()?;
    let rows = (y0..y1)
        .map(|row| axis(row - y, height, source_height, sh - 1))
        .collect::<Option<Vec<_>>>()?;
    let stride = out_w.checked_mul(3)?;
    let left = usize::try_from(x0).ok()?.checked_mul(3)?;
    let top = usize::try_from(y0).ok()?;
    let content_rows = rgb.get_mut(top.checked_mul(stride)?..)?;
    let place = Place {
        stride,
        left,
        source_width: sw,
        columns: &columns,
        rows: &rows,
    };
    if pixels.iter().all(|pixel| pixel[3] == 255) {
        fill_rows(content_rows, &place, pixels, |pixel: [u8; 4]| {
            [pixel[0], pixel[1], pixel[2]]
        });
    } else {
        let composited: Vec<[u8; 3]> = pixels.iter().map(|pixel| composite_pixel(*pixel)).collect();
        fill_rows(content_rows, &place, &composited, |pixel: [u8; 3]| pixel);
    }
    Some(rgb)
}

/// Where [`fill_rows`] puts the picture: the output's bytes a row and the picture's first byte in a row, the
/// source's width, and for every column and row of the picture the two source positions it lies between and
/// its weight.
struct Place<'a> {
    stride: usize,
    left: usize,
    source_width: usize,
    columns: &'a [(usize, usize, u32)],
    rows: &'a [(usize, usize, u32)],
}

/// The fewest rows worth a thread of their own: starting one costs about as much as scaling a few dozen rows.
const ROWS_PER_WORKER: usize = 48;

/// Fills the picture's rows of the output (`content`, from the picture's first row on) from `source`, whose
/// pixels `colour` turns into RGB; the rows are shared out between threads.
fn fill_rows<P: Copy + Sync>(
    content: &mut [u8],
    place: &Place<'_>,
    source: &[P],
    colour: impl Fn(P) -> [u8; 3] + Sync,
) {
    let sw = place.source_width;
    let render_row = |row: &mut [u8], (y0, y1, fy): (usize, usize, u32)| {
        let (Some(above), Some(below)) = (
            source.get(y0 * sw..(y0 + 1) * sw),
            source.get(y1 * sw..(y1 + 1) * sw),
        ) else {
            return;
        };
        let Some(out) = row.get_mut(place.left..) else {
            return;
        };
        let (out, _) = out.as_chunks_mut::<3>();
        for (slot, &(x0, x1, fx)) in out.iter_mut().zip(place.columns) {
            let (Some(c00), Some(c01), Some(c10), Some(c11)) =
                (above.get(x0), above.get(x1), below.get(x0), below.get(x1))
            else {
                continue;
            };
            *slot = interpolate_rgb(
                colour(*c00),
                colour(*c01),
                colour(*c10),
                colour(*c11),
                fx,
                fy,
            );
        }
    };
    let rows = place.rows;
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .clamp(1, (rows.len() / ROWS_PER_WORKER).max(1));
    let per_worker = rows.len().div_ceil(workers).max(1);
    if workers == 1 {
        for (row, weights) in content.chunks_mut(place.stride.max(1)).zip(rows) {
            render_row(row, *weights);
        }
        return;
    }
    std::thread::scope(|scope| {
        let mut rest: &mut [u8] = content;
        for block in rows.chunks(per_worker) {
            let bytes = block.len().saturating_mul(place.stride).min(rest.len());
            let (mine, after) = std::mem::take(&mut rest).split_at_mut(bytes);
            rest = after;
            let render_row = &render_row;
            let _ = scope.spawn(move || {
                for (row, weights) in mine.chunks_mut(place.stride.max(1)).zip(block) {
                    render_row(row, *weights);
                }
            });
        }
    });
}

/// Map an output coordinate to a fixed-point source coordinate.
fn source_coordinate(output_pos: u32, origin: u32, content_size: u32, source_size: u32) -> u64 {
    if content_size <= 1 || source_size <= 1 {
        return 0;
    }
    let offset = output_pos.saturating_sub(origin);
    let max_source = source_size.saturating_sub(1);
    let denom = content_size.checked_sub(1).unwrap_or(1);
    if max_source == 0 || denom == 0 {
        return 0;
    }
    let scaled = u128::from(offset)
        .checked_mul(u128::from(max_source))
        .and_then(|value| value.checked_mul(u128::from(FIXED_POINT_ONE)))
        .unwrap_or(0);
    let value = scaled / u128::from(denom);
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// Sample RGBA bilinearly and composite alpha onto black (the reference [`render_rgb`] matches).
#[cfg(test)]
fn sample_bilinear(
    rgba: &[u8],
    source_width: u32,
    source_height: u32,
    sx: u64,
    sy: u64,
) -> Option<[u8; 3]> {
    let width = usize::try_from(source_width).ok()?;
    let height = usize::try_from(source_height).ok()?;
    if width == 0 || height == 0 {
        return None;
    }

    let x0 = usize::try_from(sx >> FIXED_POINT_SHIFT)
        .unwrap_or(width.saturating_sub(1))
        .min(width.saturating_sub(1));
    let y0 = usize::try_from(sy >> FIXED_POINT_SHIFT)
        .unwrap_or(height.saturating_sub(1))
        .min(height.saturating_sub(1));
    let x1 = x0.checked_add(1).unwrap_or(x0).min(width.saturating_sub(1));
    let y1 = y0
        .checked_add(1)
        .unwrap_or(y0)
        .min(height.saturating_sub(1));

    let fx = u32::try_from(sx & (FIXED_POINT_ONE - 1)).ok()?;
    let fy = u32::try_from(sy & (FIXED_POINT_ONE - 1)).ok()?;

    let p00 = pixel_at(rgba, width, x0, y0)?;
    let p01 = pixel_at(rgba, width, x1, y0)?;
    let p10 = pixel_at(rgba, width, x0, y1)?;
    let p11 = pixel_at(rgba, width, x1, y1)?;

    let c00 = composite_pixel(p00);
    let c01 = composite_pixel(p01);
    let c10 = composite_pixel(p10);
    let c11 = composite_pixel(p11);
    Some(interpolate_rgb(c00, c01, c10, c11, fx, fy))
}

/// Read one RGBA pixel.
#[cfg(test)]
fn pixel_at(rgba: &[u8], width: usize, x: usize, y: usize) -> Option<[u8; 4]> {
    let index = y.checked_mul(width)?.checked_add(x)?.checked_mul(4)?;
    let end = index.checked_add(4)?;
    let slice = rgba.get(index..end)?;
    if slice.len() != 4 {
        return None;
    }
    let mut pixel = [0u8; 4];
    pixel.copy_from_slice(slice);
    Some(pixel)
}

/// Composite an RGBA pixel onto black.
fn composite_pixel(pixel: [u8; 4]) -> [u8; 3] {
    let [r, g, b, a] = pixel;
    [
        composite_channel(r, a),
        composite_channel(g, a),
        composite_channel(b, a),
    ]
}

/// Composite one channel onto black.
fn composite_channel(color: u8, alpha: u8) -> u8 {
    let value = (u32::from(color) * u32::from(alpha) + 127) / 255;
    u8::try_from(value).unwrap_or(255)
}

/// Bilinearly interpolate RGB channels.
fn interpolate_rgb(
    c00: [u8; 3],
    c01: [u8; 3],
    c10: [u8; 3],
    c11: [u8; 3],
    fx: u32,
    fy: u32,
) -> [u8; 3] {
    let [r00, g00, b00] = c00;
    let [r01, g01, b01] = c01;
    let [r10, g10, b10] = c10;
    let [r11, g11, b11] = c11;

    [
        interpolate_channel(r00, r01, r10, r11, fx, fy),
        interpolate_channel(g00, g01, g10, g11, fx, fy),
        interpolate_channel(b00, b01, b10, b11, fx, fy),
    ]
}

/// Bilinearly interpolate one channel.
fn interpolate_channel(c00: u8, c01: u8, c10: u8, c11: u8, fx: u32, fy: u32) -> u8 {
    let top =
        (u32::from(c00) * (FIXED_POINT_ONE_U32 - fx) + u32::from(c01) * fx) >> FIXED_POINT_SHIFT;
    let bottom =
        (u32::from(c10) * (FIXED_POINT_ONE_U32 - fx) + u32::from(c11) * fx) >> FIXED_POINT_SHIFT;
    let value = (top * (FIXED_POINT_ONE_U32 - fy) + bottom * fy) >> FIXED_POINT_SHIFT;
    u8::try_from(value).unwrap_or(255)
}

/// Write one RGB pixel into the output buffer.
#[cfg(test)]
fn write_rgb(rgb: &mut [u8], output_width: u32, y: u32, x: u32, pixel: [u8; 3]) -> Option<()> {
    let width = usize::try_from(output_width).ok()?;
    let row = usize::try_from(y).ok()?.checked_mul(width)?;
    let col = usize::try_from(x).ok()?;
    let index = row.checked_add(col)?.checked_mul(3)?;
    let end = index.checked_add(3)?;
    let slot = rgb.get_mut(index..end)?;
    if slot.len() != 3 {
        return None;
    }
    slot.copy_from_slice(&pixel);
    Some(())
}

/// Convert RGB24 to YUV 4:2:0 BT.709 limited range.
fn rgb_to_yuv(rgb: &[u8], options: &HdOptions) -> Result<YuvPlanes, TranscodeError> {
    rgb_to_yuv_as(rgb, options, false)
}

/// [`rgb_to_yuv`]; `fields`: each chroma line from the two lines of its own field around it (4:2:0 of an interlaced
/// frame: the chroma lines alternate between the fields, 13818-2 6.1.1.8 / H.264 6.2), so the colours of two
/// moments do not mix.
fn rgb_to_yuv_as(
    rgb: &[u8],
    options: &HdOptions,
    fields: bool,
) -> Result<YuvPlanes, TranscodeError> {
    let width = usize::try_from(options.width)
        .map_err(|_| TranscodeError::Unsupported("output width too large".to_string()))?;
    let height = usize::try_from(options.height)
        .map_err(|_| TranscodeError::Unsupported("output height too large".to_string()))?;
    let chroma_width = width / 2;
    let chroma_height = height / 2;
    let y_size = width
        .checked_mul(height)
        .ok_or_else(|| TranscodeError::Unsupported("luma plane too large".to_string()))?;
    let uv_size = chroma_width
        .checked_mul(chroma_height)
        .ok_or_else(|| TranscodeError::Unsupported("chroma plane too large".to_string()))?;

    let mut y = vec![0u8; y_size];
    let mut u = vec![0u8; uv_size];
    let mut v = vec![0u8; uv_size];

    fill_luma(&mut y, rgb, width, height)
        .ok_or_else(|| TranscodeError::Unsupported("failed to build luma plane".to_string()))?;
    fill_chroma(&mut u, &mut v, rgb, (width, height), fields)
        .ok_or_else(|| TranscodeError::Unsupported("failed to build chroma planes".to_string()))?;

    Ok(YuvPlanes {
        y,
        u,
        v,
        y_stride: width,
        uv_stride: chroma_width,
    })
}

/// Rows of a picture each thread of the colour conversion takes at least (M12.41): the conversion runs on the thread
/// that feeds the encoder, so a hardware encoder waits on it unless it uses the other cores too.
const CONVERT_ROWS_PER_WORKER: usize = 64;

/// How many threads convert a picture of `rows` rows.
fn convert_workers(rows: usize) -> usize {
    std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .clamp(1, (rows / CONVERT_ROWS_PER_WORKER).max(1))
}

/// Fill the luma plane (BT.709, [`luma_u8`]), the rows shared out among threads.
fn fill_luma(y: &mut [u8], rgb: &[u8], width: usize, height: usize) -> Option<()> {
    fill_luma_on(y, rgb, (width, height), convert_workers(height))
}

/// [`fill_luma`] on `workers` threads. Each luma value depends on its own pixel only, so the plane is the same
/// however the rows are shared out.
fn fill_luma_on(
    y: &mut [u8],
    rgb: &[u8],
    (width, height): (usize, usize),
    workers: usize,
) -> Option<()> {
    let row_bytes = width.checked_mul(3)?;
    let rgb = rgb.get(..row_bytes.checked_mul(height)?)?;
    let y = y.get_mut(..width.checked_mul(height)?)?;
    if width == 0 || height == 0 {
        return Some(());
    }
    let convert = |out: &mut [u8], input: &[u8]| {
        for (out_row, in_row) in out
            .chunks_exact_mut(width)
            .zip(input.chunks_exact(row_bytes))
        {
            for (value, &[r, g, b]) in out_row.iter_mut().zip(in_row.as_chunks::<3>().0) {
                *value = luma_u8([r, g, b]);
            }
        }
    };
    let per_worker = height.div_ceil(workers.max(1)).max(1);
    if per_worker >= height {
        convert(y, rgb);
        return Some(());
    }
    std::thread::scope(|scope| {
        for (out, input) in y
            .chunks_mut(per_worker.saturating_mul(width))
            .zip(rgb.chunks(per_worker.saturating_mul(row_bytes)))
        {
            let convert = &convert;
            let _ = scope.spawn(move || convert(out, input));
        }
    });
    Some(())
}

/// Fill the chroma planes by averaging 2x2 blocks: two lines next to each other, or (`fields`) the two lines of
/// one field (chroma line `cy` of field `cy % 2`: its lines `4 (cy / 2) + cy % 2` and two below). The chroma rows
/// are shared out among threads.
fn fill_chroma(
    u: &mut [u8],
    v: &mut [u8],
    rgb: &[u8],
    (width, height): (usize, usize),
    fields: bool,
) -> Option<()> {
    fill_chroma_on(
        (u, v),
        rgb,
        (width, height),
        fields,
        convert_workers(height / 2),
    )
}

/// [`fill_chroma`] on `workers` threads (each chroma value depends on its own four pixels only).
fn fill_chroma_on(
    (u, v): (&mut [u8], &mut [u8]),
    rgb: &[u8],
    (width, height): (usize, usize),
    fields: bool,
    workers: usize,
) -> Option<()> {
    let chroma_width = width / 2;
    let chroma_height = height / 2;
    let row_bytes = width.checked_mul(3)?;
    let rgb = rgb.get(..row_bytes.checked_mul(height)?)?;
    let size = chroma_width.checked_mul(chroma_height)?;
    let (u, v) = (u.get_mut(..size)?, v.get_mut(..size)?);
    if chroma_width == 0 || chroma_height == 0 {
        return Some(());
    }
    // The two picture lines chroma line `cy` is made from.
    let lines = |cy: usize| -> Option<(&[u8], &[u8])> {
        let (y0, y1) = if fields {
            let first = (cy / 2).checked_mul(4)?.checked_add(cy % 2)?;
            let second = first.checked_add(2)?;
            (first, if second < height { second } else { first })
        } else {
            let first = cy.checked_mul(2)?;
            (first, first.checked_add(1)?)
        };
        let line = |y: usize| {
            rgb.get(y.checked_mul(row_bytes)?..y.checked_add(1)?.checked_mul(row_bytes)?)
        };
        Some((line(y0)?, line(y1)?))
    };
    // Chroma rows from `first_row` on, into `u` / `v` (whole rows).
    let convert = |first_row: usize, u: &mut [u8], v: &mut [u8]| -> Option<()> {
        for (index, (u_row, v_row)) in u
            .chunks_exact_mut(chroma_width)
            .zip(v.chunks_exact_mut(chroma_width))
            .enumerate()
        {
            let (top, bottom) = lines(first_row.checked_add(index)?)?;
            let pairs = top.as_chunks::<6>().0.iter().zip(bottom.as_chunks::<6>().0);
            for (
                (cb_out, cr_out),
                (&[r00, g00, b00, r01, g01, b01], &[r10, g10, b10, r11, g11, b11]),
            ) in u_row.iter_mut().zip(v_row.iter_mut()).zip(pairs)
            {
                let avg = average_rgb(
                    [r00, g00, b00],
                    [r01, g01, b01],
                    [r10, g10, b10],
                    [r11, g11, b11],
                );
                let (cb, cr) = chroma_u8(avg);
                *cb_out = cb;
                *cr_out = cr;
            }
        }
        Some(())
    };
    let per_worker = chroma_height.div_ceil(workers.max(1)).max(1);
    if per_worker >= chroma_height {
        return convert(0, u, v);
    }
    let block = per_worker.saturating_mul(chroma_width);
    std::thread::scope(|scope| {
        let handles: Vec<_> = u
            .chunks_mut(block)
            .zip(v.chunks_mut(block))
            .enumerate()
            .map(|(index, (u, v))| {
                let convert = &convert;
                scope.spawn(move || convert(index.saturating_mul(per_worker), u, v))
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().ok().flatten())
            .collect::<Option<Vec<()>>>()
            .map(|_| ())
    })
}

/// The luma plane as it was filled before M12.41, one pixel at a time (the reference the faster one is checked
/// against).
#[cfg(test)]
fn fill_luma_reference(y: &mut [u8], rgb: &[u8], width: usize, height: usize) -> Option<()> {
    for row in 0..height {
        for col in 0..width {
            let pixel = get_rgb(rgb, width, col, row)?;
            let value = luma_u8(pixel);
            let index = row.checked_mul(width)?.checked_add(col)?;
            *y.get_mut(index)? = value;
        }
    }
    Some(())
}

/// The chroma planes as they were filled before M12.41 (the reference), averaging 2x2 blocks: two lines next to each other, or (`fields`) the two lines of
/// one field (chroma line `cy` of field `cy % 2`: its lines `4 (cy / 2) + cy % 2` and two below).
#[cfg(test)]
fn fill_chroma_reference(
    u: &mut [u8],
    v: &mut [u8],
    rgb: &[u8],
    (width, height): (usize, usize),
    fields: bool,
) -> Option<()> {
    let chroma_width = width / 2;
    let chroma_height = height / 2;
    for cy in 0..chroma_height {
        let (y0, y1) = if fields {
            let first = (cy / 2).checked_mul(4)?.checked_add(cy % 2)?;
            let second = first.checked_add(2)?;
            (first, if second < height { second } else { first })
        } else {
            let first = cy.checked_mul(2)?;
            (first, first.checked_add(1)?)
        };
        for cx in 0..chroma_width {
            let x0 = cx.checked_mul(2)?;
            let p00 = get_rgb(rgb, width, x0, y0)?;
            let p01 = get_rgb(rgb, width, x0.checked_add(1)?, y0)?;
            let p10 = get_rgb(rgb, width, x0, y1)?;
            let p11 = get_rgb(rgb, width, x0.checked_add(1)?, y1)?;
            let avg = average_rgb(p00, p01, p10, p11);
            let (cb, cr) = chroma_u8(avg);
            let index = cy.checked_mul(chroma_width)?.checked_add(cx)?;
            *u.get_mut(index)? = cb;
            *v.get_mut(index)? = cr;
        }
    }
    Some(())
}

/// Read one RGB pixel.
#[cfg(test)]
fn get_rgb(rgb: &[u8], width: usize, x: usize, y: usize) -> Option<[u8; 3]> {
    let index = y.checked_mul(width)?.checked_add(x)?.checked_mul(3)?;
    let end = index.checked_add(3)?;
    let slice = rgb.get(index..end)?;
    if slice.len() != 3 {
        return None;
    }
    let mut pixel = [0u8; 3];
    pixel.copy_from_slice(slice);
    Some(pixel)
}

/// Average four RGB pixels.
fn average_rgb(p00: [u8; 3], p01: [u8; 3], p10: [u8; 3], p11: [u8; 3]) -> [u8; 3] {
    let [r00, g00, b00] = p00;
    let [r01, g01, b01] = p01;
    let [r10, g10, b10] = p10;
    let [r11, g11, b11] = p11;
    [
        average_channel(r00, r01, r10, r11),
        average_channel(g00, g01, g10, g11),
        average_channel(b00, b01, b10, b11),
    ]
}

/// Average four channel values with rounding.
fn average_channel(a: u8, b: u8, c: u8, d: u8) -> u8 {
    let sum = u32::from(a) + u32::from(b) + u32::from(c) + u32::from(d);
    let avg = (sum + 2) / 4;
    u8::try_from(avg).unwrap_or(255)
}

/// Compute BT.709 limited-range luma.
fn luma_u8(pixel: [u8; 3]) -> u8 {
    let [r, g, b] = pixel;
    let luma = 0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b);
    let value = 16.0 + 219.0 * luma / 255.0;
    round_clamp_u8(value)
}

/// Compute BT.709 limited-range chroma from averaged RGB.
fn chroma_u8(pixel: [u8; 3]) -> (u8, u8) {
    let [r, g, b] = pixel;
    let rf = f64::from(r);
    let gf = f64::from(g);
    let bf = f64::from(b);
    let yprime = 0.2126 * rf + 0.7152 * gf + 0.0722 * bf;
    let cb = 128.0 + 224.0 * (bf - yprime) / (1.8556 * 255.0);
    let cr = 128.0 + 224.0 * (rf - yprime) / (1.5748 * 255.0);
    (round_clamp_u8(cb), round_clamp_u8(cr))
}

/// Round a floating-point value and clamp it to u8.
fn round_clamp_u8(value: f64) -> u8 {
    let rounded = value.round();
    if rounded <= 0.0 {
        return 0;
    }
    if rounded >= 255.0 {
        return 255;
    }
    // In range 1..=254 after the checks above, so the conversion is exact.
    rounded as u8
}

/// Write an access-unit index file, as [`read_h264_index`] reads it: an encoded stream's, or one demuxed as it is.
pub fn write_h264_index(path: &Path, units: &[AccessUnit]) -> Result<(), TranscodeError> {
    let file = File::create(path).map_err(|error| io_error(path, error))?;
    let mut writer = BufWriter::new(file);
    writer
        .write_all(MAGIC)
        .map_err(|error| io_error(path, error))?;
    let count = u32::try_from(units.len())
        .map_err(|_| TranscodeError::Unsupported("too many access units".to_string()))?;
    writer
        .write_all(&count.to_le_bytes())
        .map_err(|error| io_error(path, error))?;
    for unit in units {
        write_index_entry(&mut writer, unit).map_err(|error| io_error(path, error))?;
    }
    writer.flush().map_err(|error| io_error(path, error))?;
    Ok(())
}

/// Write one index entry.
fn write_index_entry<W: Write>(writer: &mut W, unit: &AccessUnit) -> std::io::Result<()> {
    writer.write_all(&unit.offset.to_le_bytes())?;
    writer.write_all(&unit.len.to_le_bytes())?;
    writer.write_all(&unit.pts.to_le_bytes())?;
    writer.write_all(&unit.dts.to_le_bytes())?;
    writer.write_all(&[u8::from(unit.keyframe)])?;
    Ok(())
}

/// Parse one index entry.
fn parse_entry(entry: &[u8]) -> Result<AccessUnit, TranscodeError> {
    let offset = read_u64_at(entry, 0)?;
    let len = read_u32_at(entry, 8)?;
    let pts = read_i64_at(entry, 12)?;
    let dts = read_i64_at(entry, 20)?;
    let key = entry
        .get(28)
        .copied()
        .ok_or_else(|| TranscodeError::Unsupported("index entry is truncated".to_string()))?;
    Ok(AccessUnit {
        offset,
        len,
        pts,
        dts,
        keyframe: key != 0,
    })
}

/// Read a little-endian u64 at a byte offset.
fn read_u64_at(bytes: &[u8], start: usize) -> Result<u64, TranscodeError> {
    let end = start
        .checked_add(8)
        .ok_or_else(|| TranscodeError::Unsupported("index offset overflow".to_string()))?;
    let slice = bytes
        .get(start..end)
        .ok_or_else(|| TranscodeError::Unsupported("index entry is truncated".to_string()))?;
    let array = <[u8; 8]>::try_from(slice)
        .map_err(|_| TranscodeError::Unsupported("index entry is truncated".to_string()))?;
    Ok(u64::from_le_bytes(array))
}

/// Read a little-endian u32 at a byte offset.
fn read_u32_at(bytes: &[u8], start: usize) -> Result<u32, TranscodeError> {
    let end = start
        .checked_add(4)
        .ok_or_else(|| TranscodeError::Unsupported("index offset overflow".to_string()))?;
    let slice = bytes
        .get(start..end)
        .ok_or_else(|| TranscodeError::Unsupported("index entry is truncated".to_string()))?;
    let array = <[u8; 4]>::try_from(slice)
        .map_err(|_| TranscodeError::Unsupported("index entry is truncated".to_string()))?;
    Ok(u32::from_le_bytes(array))
}

/// Read a little-endian i64 at a byte offset.
fn read_i64_at(bytes: &[u8], start: usize) -> Result<i64, TranscodeError> {
    let end = start
        .checked_add(8)
        .ok_or_else(|| TranscodeError::Unsupported("index offset overflow".to_string()))?;
    let slice = bytes
        .get(start..end)
        .ok_or_else(|| TranscodeError::Unsupported("index entry is truncated".to_string()))?;
    let array = <[u8; 8]>::try_from(slice)
        .map_err(|_| TranscodeError::Unsupported("index entry is truncated".to_string()))?;
    Ok(i64::from_le_bytes(array))
}

/// Report progress every ten frames.
fn report_progress(progress: &mut dyn FnMut(f32), index: u64, frames: u64) {
    let done = index.checked_add(1).unwrap_or(frames);
    if done == 0 || !done.is_multiple_of(PROGRESS_INTERVAL) {
        return;
    }
    progress(progress_fraction(done, frames));
}

/// Compute a clamped progress fraction.
fn progress_fraction(done: u64, frames: u64) -> f32 {
    if frames == 0 {
        return 1.0;
    }
    let done_f = f64::from(u32::try_from(done).unwrap_or(u32::MAX));
    let frames_f = f64::from(u32::try_from(frames).unwrap_or(u32::MAX));
    let fraction = if frames_f == 0.0 {
        1.0
    } else {
        done_f / frames_f
    };
    fraction.clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{
        AccessUnit, HdOptions, chroma_u8, fill_chroma_on, fill_chroma_reference, fill_luma_on,
        fill_luma_reference, frame_count, read_h264_index, rgb_to_yuv, rgb_to_yuv_as,
        target_geometry, write_h264_index,
    };

    /// M12.41: the colour conversion shared out among threads makes the same planes as the one that went one pixel
    /// at a time, for any number of threads, odd sizes and both chroma line orders (frame and fields).
    #[test]
    fn the_parallel_colour_conversion_matches_the_reference() {
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 24) as u8
        };
        for (width, height) in [(1, 1), (2, 2), (3, 5), (17, 9), (64, 130), (321, 241)] {
            let rgb: Vec<u8> = (0..width * height * 3).map(|_| next()).collect();
            let mut expected = vec![0; width * height];
            fill_luma_reference(&mut expected, &rgb, width, height).unwrap();
            let size = (width / 2) * (height / 2);
            for workers in [1, 2, 3, 7, 64] {
                let mut luma = vec![0; width * height];
                fill_luma_on(&mut luma, &rgb, (width, height), workers).unwrap();
                assert_eq!(luma, expected, "{width}x{height} on {workers}");
                for fields in [false, true] {
                    let (mut u, mut v) = (vec![0; size], vec![0; size]);
                    fill_chroma_reference(&mut u, &mut v, &rgb, (width, height), fields).unwrap();
                    let (mut cb, mut cr) = (vec![0; size], vec![0; size]);
                    fill_chroma_on((&mut cb, &mut cr), &rgb, (width, height), fields, workers)
                        .unwrap();
                    assert_eq!(
                        (cb, cr),
                        (u, v),
                        "{width}x{height} fields {fields} on {workers}"
                    );
                }
            }
        }
        // A picture shorter than its size says is refused, as before.
        let mut luma = vec![0; 4];
        assert!(fill_luma_on(&mut luma, &[0; 9], (2, 2), 2).is_none());
    }
    use crate::TICKS_PER_SECOND;
    use craft_video::h264::{
        Encoder, EncoderConfig, PacketFormat, Preset, Profile, RateControl, YuvFrame,
    };
    use filmcraft_time::Tick;
    use std::fs;
    use std::path::PathBuf;

    /// Create a temporary file path for tests.
    fn temp_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "disccraft_hd_{}_{}_{}.bin",
            name,
            std::process::id(),
            nanos
        ))
    }

    /// Default options for small tests.
    fn test_options(width: u32, height: u32) -> HdOptions {
        HdOptions {
            width,
            height,
            fps_num: 24,
            fps_den: 1,
            target_kbps: 1000,
            max_kbps: 2000,
            codec: super::HdCodec::H264,
            quality: crate::BEST_QUALITY,
            rendering: crate::Rendering::default(),
            mode: crate::RateMode::Vbr,
            interlaced: false,
            wide: false,
        }
    }

    /// 16:9 source fills a 16:9 HD frame.
    #[test]
    fn geometry_16_9_full() {
        let geometry = target_geometry(1280, 720, (1, 1), 1920, (1080, false)).unwrap();
        assert_eq!(
            (geometry.x, geometry.y, geometry.width, geometry.height),
            (0, 0, 1920, 1080)
        );
    }

    /// M12.26: scaled as fields, each output line takes only lines of its own field: a picture whose top field is
    /// white and bottom field black gives white even lines and black odd ones (scaled as a frame, every line is a
    /// grey mix of the two).
    #[test]
    fn fields_are_scaled_each_on_its_own() {
        let (sw, sh) = (64_u32, 108_u32);
        let rgba: Vec<u8> = (0..sh)
            .flat_map(|row| {
                let value = if row % 2 == 0 { 255 } else { 0 };
                (0..sw).flat_map(move |_| [value, value, value, 255])
            })
            .collect();
        let fields =
            super::render_rgb_fields_at(&rgba, (sw, sh), (0, 0, 32, 48), (32, 48)).unwrap();
        let frame = super::render_rgb_at(&rgba, (sw, sh), (0, 0, 32, 48), (32, 48)).unwrap();
        for row in 0..48_usize {
            let value = fields[row * 32 * 3];
            if row % 2 == 0 {
                assert_eq!(value, 255, "row {row}");
            } else {
                assert_eq!(value, 0, "row {row}");
            }
        }
        assert!(
            (1..47).any(|row: usize| (20..235).contains(&frame[row * 32 * 3])),
            "scaled as a frame, the fields mix"
        );
        // A letterboxed picture starting on an odd line: the lines keep their field.
        let boxed = super::render_rgb_fields_at(&rgba, (sw, sh), (0, 7, 32, 33), (32, 48)).unwrap();
        for row in 7..40_usize {
            let expected = if row % 2 == 0 { 255 } else { 0 };
            assert_eq!(boxed[row * 32 * 3], expected, "boxed row {row}");
        }
    }

    /// M6.393: HDV's 1440 x 1080 is shown 16:9: a 16:9 source fills it, a 4:3 one is pillarboxed to 1080 pixels
    /// (1440 shown).
    #[test]
    fn geometry_of_the_hdv_frame() {
        let full = target_geometry(1920, 1080, (1, 1), 1440, (1080, false)).unwrap();
        assert_eq!(
            (full.x, full.y, full.width, full.height),
            (0, 0, 1440, 1080)
        );
        let boxed = target_geometry(640, 480, (1, 1), 1440, (1080, false)).unwrap();
        assert_eq!(
            (boxed.x, boxed.y, boxed.width, boxed.height),
            (180, 0, 1080, 1080)
        );
    }

    /// 4:3 source is pillarboxed in a 16:9 HD frame.
    #[test]
    fn geometry_4_3_pillarbox() {
        let geometry = target_geometry(640, 480, (1, 1), 1920, (1080, false)).unwrap();
        assert_eq!(
            (geometry.x, geometry.y, geometry.width, geometry.height),
            (240, 0, 1440, 1080)
        );
    }

    /// Anamorphic DVD 16:9 source fills HD width.
    #[test]
    fn geometry_dvd_16_9_full_width() {
        let geometry = target_geometry(720, 480, (40, 33), 1920, (1080, false)).unwrap();
        assert_eq!(geometry.width, 1920);
        assert!(geometry.height >= 1050 && geometry.height <= 1080);
    }

    /// White RGB maps to BT.709 limited-range white.
    #[test]
    fn yuv_white() {
        let rgb = vec![255u8; 2 * 2 * 3];
        let planes = rgb_to_yuv(&rgb, &test_options(2, 2)).unwrap();
        assert_eq!(planes.y[0], 235);
        assert_eq!(planes.u[0], 128);
        assert_eq!(planes.v[0], 128);
    }

    /// Black RGB maps to BT.709 limited-range black.
    #[test]
    fn yuv_black() {
        let rgb = vec![0u8; 2 * 2 * 3];
        let planes = rgb_to_yuv(&rgb, &test_options(2, 2)).unwrap();
        assert_eq!(planes.y[0], 16);
        assert_eq!(planes.u[0], 128);
        assert_eq!(planes.v[0], 128);
    }

    /// M12.37: the chroma of an interlaced frame whose fields stay apart is subsampled within each field: with the
    /// top field red and the bottom field blue, the chroma lines alternate red and blue, where whole frames mix
    /// them into one purple.
    #[test]
    fn yuv_of_fields_keeps_each_fields_colour() {
        let (width, height) = (4, 8);
        let rgb: Vec<u8> = (0..width * height)
            .flat_map(|index| {
                if (index / width) % 2 == 0 {
                    [255, 0, 0]
                } else {
                    [0, 0, 255]
                }
            })
            .collect();
        let options = test_options(4, 8);
        let fields = rgb_to_yuv_as(&rgb, &options, true).unwrap();
        let (red, blue) = (chroma_u8([255, 0, 0]), chroma_u8([0, 0, 255]));
        for line in 0..4 {
            let expected = if line % 2 == 0 { red } else { blue };
            assert_eq!(
                (fields.u[line * 2], fields.v[line * 2]),
                expected,
                "line {line}"
            );
        }
        let frames = rgb_to_yuv(&rgb, &options).unwrap();
        assert!((0..4).all(|line| (frames.u[line * 2], frames.v[line * 2]) != red));
    }

    /// Index round-trips through write and read.
    #[test]
    fn index_round_trip() {
        let path = temp_path("index");
        let units = vec![
            AccessUnit {
                offset: 0,
                len: 10,
                pts: 0,
                dts: -1,
                keyframe: true,
            },
            AccessUnit {
                offset: 10,
                len: 20,
                pts: 3750,
                dts: 0,
                keyframe: false,
            },
        ];
        write_h264_index(&path, &units).unwrap();
        let read = read_h264_index(&path).unwrap();
        assert_eq!(read, units);
        let _ = fs::remove_file(&path);
    }

    /// Truncated index files are rejected.
    #[test]
    fn index_truncated() {
        let path = temp_path("truncated");
        let units = vec![AccessUnit {
            offset: 0,
            len: 1,
            pts: 0,
            dts: 0,
            keyframe: true,
        }];
        write_h264_index(&path, &units).unwrap();
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
        let error = read_h264_index(&path).unwrap_err();
        assert!(matches!(error, crate::TranscodeError::Unsupported(_)));
        let _ = fs::remove_file(&path);
    }

    /// Six seconds at 24000/1001 yields 144 frames (143.86, the nearest).
    #[test]
    fn a_range_is_cut_to_the_source() {
        let total = Tick(10 * TICKS_PER_SECOND);
        let at = |seconds: i64| Tick(seconds * TICKS_PER_SECOND);
        assert_eq!(
            super::source_range(total, (Tick(0), None)),
            (Tick(0), total)
        );
        assert_eq!(
            super::source_range(total, (at(2), Some(at(5)))),
            (at(2), at(3))
        );
        assert_eq!(
            super::source_range(total, (at(8), Some(at(20)))),
            (at(8), at(2))
        );
        assert_eq!(super::source_range(total, (at(12), None)), (total, Tick(0)));
        assert_eq!(
            super::source_range(total, (at(5), Some(at(3)))),
            (at(5), Tick(0))
        );
    }

    /// Six seconds at 23.976 are 144 frames.
    #[test]
    fn frame_count_six_seconds() {
        let ticks = 6 * TICKS_PER_SECOND;
        let options = HdOptions {
            width: 1920,
            height: 1080,
            fps_num: 24000,
            fps_den: 1001,
            target_kbps: 1000,
            max_kbps: 2000,
            codec: super::HdCodec::H264,
            quality: crate::BEST_QUALITY,
            rendering: crate::Rendering::default(),
            mode: crate::RateMode::Vbr,
            interlaced: false,
            wide: false,
        };
        assert_eq!(frame_count(Tick(ticks), &options).unwrap(), 144);
    }

    /// Direct encoder smoke test produces at least one keyframe.
    #[test]
    fn encoder_produces_keyframe() {
        let mut config = EncoderConfig::new(64, 64, 24, 1);
        config.profile = Profile::High;
        config.preset = Preset::Balanced;
        config.rate = RateControl::Vbr {
            target_kbps: 1000,
            max_kbps: 2000,
        };
        config.keyint = 24;
        config.bframes = 2;
        config.slices = 4;
        config.aud = true;
        config.level = Some(41);
        config.format = PacketFormat::AnnexB;

        let mut encoder = Encoder::new(config).unwrap();
        let y = vec![16u8; 64 * 64];
        let u = vec![128u8; 32 * 32];
        let v = vec![128u8; 32 * 32];
        let mut keyframes = 0;

        for pts in 0i64..3 {
            let frame = YuvFrame {
                y: &y,
                u: &u,
                v: &v,
                y_stride: 64,
                uv_stride: 32,
            };
            let packets = encoder.encode(&frame, pts).unwrap();
            for packet in packets {
                if packet.keyframe {
                    keyframes += 1;
                }
            }
        }
        for packet in encoder.flush() {
            if packet.keyframe {
                keyframes += 1;
            }
        }
        assert!(keyframes >= 1);
    }

    fn mpeg2_options(fps_num: u32, fps_den: u32) -> HdOptions {
        HdOptions {
            width: 1920,
            height: 1080,
            fps_num,
            fps_den,
            target_kbps: 25_000,
            max_kbps: 35_000,
            codec: super::HdCodec::Mpeg2,
            quality: crate::BEST_QUALITY,
            rendering: crate::Rendering::default(),
            mode: crate::RateMode::Vbr,
            interlaced: false,
            wide: false,
        }
    }

    /// M10.14: with the MPEG-2 codec the pictures become an MP@HL sequence, one access unit a picture, a key frame
    /// where a GOP starts with its sequence header. M12.33: in decoding order, two B pictures between the reference
    /// pictures (each shown at its place, decoded a frame before its place in decoding order).
    #[test]
    fn mpeg2_frames_are_one_unit_a_picture() {
        let options = mpeg2_options(25, 1);
        let mut frames = super::H264Frames::new(&options).unwrap();
        let mut rgb = vec![40_u8; 1920 * 1080 * 3];
        for index in 0..13_usize {
            // Something moves, so the P pictures are not empty.
            for value in rgb.iter_mut().skip(index * 3000).take(3000) {
                *value = 200;
            }
            frames.push_rgb(&rgb).unwrap();
        }
        let (data, units) = frames.finish().unwrap();
        assert_eq!(units.len(), 13);
        // I0 P3 B1 B2 P6 B4 B5 P9 B7 B8, then (the GOP of 12 closed) P11 B10, and I12.
        let shown = [0, 3, 1, 2, 6, 4, 5, 9, 7, 8, 11, 10, 12];
        for (index, (unit, place)) in units.iter().zip(shown).enumerate() {
            // 25 frames a second: 3600 ticks of 90 kHz a frame.
            assert_eq!(unit.pts, place * 3600, "unit {index}");
            assert_eq!(unit.dts, (index as i64 - 1) * 3600, "unit {index}");
            assert!(unit.dts <= unit.pts);
            // A GOP of 12 at 25 frames a second.
            assert_eq!(unit.keyframe, place % 12 == 0, "unit {index}");
            let start = unit.offset as usize;
            let bytes = &data[start..start + unit.len as usize];
            let expected: &[u8] = if unit.keyframe {
                // Sequence header: 1920 x 1080, 16:9, 25 frames a second.
                &[0, 0, 1, 0xB3, 0x78, 0x04, 0x38, 0x33]
            } else {
                &[0, 0, 1, 0x00]
            };
            assert!(bytes.starts_with(expected), "unit {index}");
        }
        assert_eq!(
            units.last().map(|unit| unit.offset + u64::from(unit.len)),
            Some(data.len() as u64)
        );
    }

    #[test]
    fn mpeg2_still_is_one_sequence_with_its_end() {
        let options = mpeg2_options(30_000, 1001);
        let rgb = vec![90_u8; 1920 * 1080 * 3];
        let data = super::encode_h264_still(&rgb, &options).unwrap();
        assert!(data.starts_with(&[0, 0, 1, 0xB3, 0x78, 0x04, 0x38, 0x34]));
        assert!(data.ends_with(&[0, 0, 1, 0xB7]));
        // One picture.
        let pictures = data.windows(4).filter(|w| *w == [0, 0, 1, 0]).count();
        assert_eq!(pictures, 1);
    }

    /// M10.39: MPEG-2 for the progressive formats too, as progressive sequences at their rates; only a Blu-ray's
    /// formats (not 720 lines at 25, nor 1080 at 59.94 frames).
    #[test]
    fn mpeg2_is_a_blu_rays_formats() {
        for (options, code) in [
            (mpeg2_options(24_000, 1001), 1),
            (mpeg2_options(24, 1), 2),
            (
                HdOptions {
                    width: 1280,
                    height: 720,
                    ..mpeg2_options(60_000, 1001)
                },
                7,
            ),
            (
                HdOptions {
                    width: 1280,
                    height: 720,
                    ..mpeg2_options(50, 1)
                },
                6,
            ),
            (
                HdOptions {
                    width: 1440,
                    ..mpeg2_options(24_000, 1001)
                },
                1,
            ),
        ] {
            let rgb = vec![90_u8; options.width as usize * options.height as usize * 3];
            let data = super::encode_h264_still(&rgb, &options).unwrap();
            assert_eq!(data[7] & 0x0F, code, "{options:?}");
            // The sequence extension: progressive_sequence.
            assert_eq!(data[17] & 0x08, 0x08, "{options:?}");
        }
        for options in [
            HdOptions {
                width: 1280,
                height: 720,
                ..mpeg2_options(25, 1)
            },
            mpeg2_options(60_000, 1001),
            mpeg2_options(50, 1),
        ] {
            assert!(super::H264Frames::new(&options).is_err(), "{options:?}");
        }
        // More than a Blu-ray's 40 Mbps of video.
        let fast = HdOptions {
            max_kbps: 45_000,
            ..mpeg2_options(25, 1)
        };
        assert!(super::H264Frames::new(&fast).is_err());
    }
}

#[cfg(test)]
mod render_tests {
    use super::{Geometry, render_rgb, sample_bilinear, source_coordinate, write_rgb};

    /// The per-pixel renderer [`render_rgb`] replaced, as the reference.
    fn reference_render(
        rgba: &[u8],
        source_width: u32,
        source_height: u32,
        geometry: Geometry,
        output_width: u32,
        output_height: u32,
    ) -> Option<Vec<u8>> {
        let width = usize::try_from(output_width).ok()?;
        let height = usize::try_from(output_height).ok()?;
        let size = width.checked_mul(height)?.checked_mul(3)?;
        let mut rgb = vec![0u8; size];

        let end_x = geometry.x.checked_add(geometry.width)?;
        let end_y = geometry.y.checked_add(geometry.height)?;
        for y in geometry.y..end_y {
            for x in geometry.x..end_x {
                let sx = source_coordinate(x, geometry.x, geometry.width, source_width);
                let sy = source_coordinate(y, geometry.y, geometry.height, source_height);
                let pixel = sample_bilinear(rgba, source_width, source_height, sx, sy)?;
                write_rgb(&mut rgb, output_width, y, x, pixel)?;
            }
        }
        Some(rgb)
    }

    /// The fast renderer gives the per-pixel one's bytes: scaled up and down, letterboxed and pillarboxed, with
    /// see-through pixels, and with the odd sizes of one row or column.
    #[test]
    fn the_fast_renderer_matches_the_per_pixel_one() {
        let mut seed = 7_u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (seed >> 16) as u8
        };
        for &(sw, sh, ow, oh, geometry) in &[
            (
                64_u32,
                36_u32,
                192_u32,
                108_u32,
                (0_u32, 0_u32, 192_u32, 108_u32),
            ),
            (200, 150, 96, 54, (12, 0, 72, 54)),
            (50, 20, 80, 60, (0, 14, 80, 32)),
            (1, 1, 7, 5, (1, 1, 5, 3)),
            (9, 1, 3, 3, (0, 1, 3, 1)),
        ] {
            let pixels = usize::try_from(sw * sh * 4).unwrap_or(0);
            let rgba: Vec<u8> = (0..pixels).map(|_| next()).collect();
            let (x, y, width, height) = geometry;
            let geometry = Geometry {
                x,
                y,
                width,
                height,
            };
            let fast = render_rgb(&rgba, sw, sh, geometry, ow, oh);
            let reference = reference_render(&rgba, sw, sh, geometry, ow, oh);
            assert!(fast.is_some());
            assert_eq!(fast, reference, "{sw}x{sh} into {ow}x{oh}");
        }
        // Too little source data is refused, not read past.
        let geometry = Geometry {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        };
        assert_eq!(render_rgb(&[0; 12], 4, 4, geometry, 4, 4), None);
    }
}
