//! HD H.264 transcode for Blu-ray.
//!
//! Decodes a supported media file, scales it to an HD frame, and encodes an
//! Annex B H.264 elementary stream plus an access-unit index.

use std::convert::TryFrom;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use filmcraft_frame::VideoFrame;
use filmcraft_h264enc::{
    Encoder, EncoderConfig, Packet, PacketFormat, Preset, Profile, RateControl, YuvFrame,
};
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
    fn write_packet(&mut self, packet: &Packet, options: &HdOptions) -> Result<(), TranscodeError> {
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
    validate_options(options)?;
    let source = Source::open(input)?;
    let frames = frame_count(source.info().duration, options)?;

    let file = File::create(es_out).map_err(|error| io_error(es_out, error))?;
    let mut writer = BufWriter::new(file);
    let mut encoder = Encoder::new(encoder_config(options))
        .map_err(|error| TranscodeError::Encode(error.to_string()))?;
    let mut packets = PacketWriter::new(&mut writer, es_out);

    for frame_index in 0..frames {
        encode_one_frame(&source, &mut encoder, frame_index, options, &mut packets)?;
        report_progress(progress, frame_index, frames);
    }

    finish_encoder(&mut encoder, options, &mut packets)?;
    packets.flush()?;
    write_index(index_out, packets.units())?;
    progress(1.0);
    Ok(frames)
}

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
    let count = if numerator == 0 {
        1
    } else {
        numerator / denominator
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
    config.rate = RateControl::Vbr {
        target_kbps: options.target_kbps,
        max_kbps: options.max_kbps,
    };
    config.keyint = keyint(options);
    config.bframes = 2;
    config.slices = 4;
    config.aud = true;
    config.level = Some(41);
    config.format = PacketFormat::AnnexB;
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
fn encode_one_frame<W: Write>(
    source: &Source,
    encoder: &mut Encoder,
    frame_index: u64,
    options: &HdOptions,
    packets: &mut PacketWriter<'_, W>,
) -> Result<(), TranscodeError> {
    let time = frame_tick(frame_index, options)?;
    let frame = source.video_frame(time)?;
    let rgb = render_rgb_frame(&frame, options)?;
    let planes = rgb_to_yuv(&rgb, options)?;
    let pts = i64::try_from(frame_index)
        .map_err(|_| TranscodeError::Unsupported("frame index too large".to_string()))?;
    let encoded = encoder
        .encode(&planes.frame(), pts)
        .map_err(|error| TranscodeError::Encode(error.to_string()))?;
    for packet in &encoded {
        packets.write_packet(packet, options)?;
    }
    Ok(())
}

/// Flush the encoder and write remaining packets.
fn finish_encoder<W: Write>(
    encoder: &mut Encoder,
    options: &HdOptions,
    packets: &mut PacketWriter<'_, W>,
) -> Result<(), TranscodeError> {
    let rest = encoder.flush();
    for packet in &rest {
        packets.write_packet(packet, options)?;
    }
    Ok(())
}

/// Render a source frame into an RGB24 HD frame.
fn render_rgb_frame(frame: &VideoFrame, options: &HdOptions) -> Result<Vec<u8>, TranscodeError> {
    let geometry = target_geometry(
        frame.width,
        frame.height,
        frame.par,
        options.width,
        options.height,
    )
    .ok_or_else(|| TranscodeError::Unsupported("invalid frame geometry".to_string()))?;
    let rgba = frame.to_rgba8();
    render_rgb(
        &rgba,
        frame.width,
        frame.height,
        geometry,
        options.width,
        options.height,
    )
    .ok_or_else(|| TranscodeError::Unsupported("failed to render frame".to_string()))
}

/// Compute letterbox/pillarbox geometry for an HD output frame.
fn target_geometry(
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

/// Render RGBA source into RGB24 output with bilinear sampling.
fn render_rgb(
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

/// Sample RGBA bilinearly and composite alpha onto black.
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
    fill_chroma(&mut u, &mut v, rgb, width, height)
        .ok_or_else(|| TranscodeError::Unsupported("failed to build chroma planes".to_string()))?;

    Ok(YuvPlanes {
        y,
        u,
        v,
        y_stride: width,
        uv_stride: chroma_width,
    })
}

/// Fill the luma plane.
fn fill_luma(y: &mut [u8], rgb: &[u8], width: usize, height: usize) -> Option<()> {
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

/// Fill the chroma planes by averaging 2x2 blocks.
fn fill_chroma(u: &mut [u8], v: &mut [u8], rgb: &[u8], width: usize, height: usize) -> Option<()> {
    let chroma_width = width / 2;
    let chroma_height = height / 2;
    for cy in 0..chroma_height {
        for cx in 0..chroma_width {
            let x0 = cx.checked_mul(2)?;
            let y0 = cy.checked_mul(2)?;
            let p00 = get_rgb(rgb, width, x0, y0)?;
            let p01 = get_rgb(rgb, width, x0.checked_add(1)?, y0)?;
            let p10 = get_rgb(rgb, width, x0, y0.checked_add(1)?)?;
            let p11 = get_rgb(rgb, width, x0.checked_add(1)?, y0.checked_add(1)?)?;
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

/// Write an access-unit index file.
fn write_index(path: &Path, units: &[AccessUnit]) -> Result<(), TranscodeError> {
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
        AccessUnit, HdOptions, frame_count, read_h264_index, rgb_to_yuv, target_geometry,
        write_index,
    };
    use crate::TICKS_PER_SECOND;
    use filmcraft_h264enc::{
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
        }
    }

    /// 16:9 source fills a 16:9 HD frame.
    #[test]
    fn geometry_16_9_full() {
        let geometry = target_geometry(1280, 720, (1, 1), 1920, 1080).unwrap();
        assert_eq!(
            (geometry.x, geometry.y, geometry.width, geometry.height),
            (0, 0, 1920, 1080)
        );
    }

    /// 4:3 source is pillarboxed in a 16:9 HD frame.
    #[test]
    fn geometry_4_3_pillarbox() {
        let geometry = target_geometry(640, 480, (1, 1), 1920, 1080).unwrap();
        assert_eq!(
            (geometry.x, geometry.y, geometry.width, geometry.height),
            (240, 0, 1440, 1080)
        );
    }

    /// Anamorphic DVD 16:9 source fills HD width.
    #[test]
    fn geometry_dvd_16_9_full_width() {
        let geometry = target_geometry(720, 480, (40, 33), 1920, 1080).unwrap();
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
        write_index(&path, &units).unwrap();
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
        write_index(&path, &units).unwrap();
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
        let error = read_h264_index(&path).unwrap_err();
        assert!(matches!(error, crate::TranscodeError::Unsupported(_)));
        let _ = fs::remove_file(&path);
    }

    /// Six seconds at 24000/1001 yields 143 frames.
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
        };
        assert_eq!(frame_count(Tick(ticks), &options).unwrap(), 143);
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
}
