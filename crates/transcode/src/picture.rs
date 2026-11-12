use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use disccraft_mpeg2venc::encoder::{Encoder, EncoderConfig};
use disccraft_mpeg2venc::{Standard, rgb_to_frame420};
use filmcraft_frame::VideoFrame;
use filmcraft_time::{FrameRate, Tick};

use crate::source::Source;
use crate::{TICKS_PER_SECOND, TranscodeError, io_error};

const OUTPUT_WIDTH: u32 = 720;
/// Display aspect of the output frame: 4:3, or 16:9 when `wide` (set per call through `Target`).
#[derive(Clone, Copy)]
pub(crate) struct Target {
    pub(crate) num: u128,
    pub(crate) den: u128,
}

pub(crate) const FOUR_THREE: Target = Target { num: 4, den: 3 };
pub(crate) const SIXTEEN_NINE: Target = Target { num: 16, den: 9 };
const FIXED_POINT_SHIFT: u32 = 16;
const FIXED_POINT_ONE: u64 = 1 << FIXED_POINT_SHIFT;

/// Rectangle where source content is placed in the DVD frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Geometry {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Compute the number of output frames for `duration`.
pub(crate) fn frame_count(duration: Tick, standard: Standard) -> Option<u64> {
    let ticks = u64::try_from(duration.0).ok()?;
    let (fps_num, fps_den) = if standard == Standard::Ntsc {
        (30_000u64, 1_001u64)
    } else {
        (25u64, 1u64)
    };

    let ticks_per_second = u64::try_from(TICKS_PER_SECOND).ok()?;
    let numerator = u128::from(ticks).checked_mul(u128::from(fps_num))?;
    let denominator = u128::from(ticks_per_second).checked_mul(u128::from(fps_den))?;

    u64::try_from(numerator / denominator).ok()
}

/// Encode all output frames into an MPEG-2 video elementary stream.
pub(crate) fn encode_video(
    source: &Source,
    output_path: &Path,
    options: &crate::TranscodeOptions,
    max_bitrate: u32,
    frames: u64,
    progress: &mut dyn FnMut(f32),
) -> Result<(), TranscodeError> {
    let standard = options.standard;
    let target = if options.wide {
        SIXTEEN_NINE
    } else {
        FOUR_THREE
    };
    let file = File::create(output_path).map_err(|error| io_error(output_path, error))?;
    let mut writer = BufWriter::new(file);
    let mut encoder = Encoder::new(encoder_config(options, max_bitrate))
        .map_err(|e| TranscodeError::Encode(e.to_string()))?;

    let output_height = output_height(standard);
    let rate = frame_rate(standard);
    let out_width = u16::try_from(OUTPUT_WIDTH)
        .map_err(|_| TranscodeError::Unsupported("output width too large".to_string()))?;
    let out_height = u16::try_from(output_height)
        .map_err(|_| TranscodeError::Unsupported("output height too large".to_string()))?;

    for index in 0..frames {
        let time = rate.tick_of(frame_index_to_i64(index)?);
        let frame = source.video_frame(time)?;
        let rgb = render_source_frame(&frame, output_height, target)?;
        let frame420 = rgb_to_frame420(out_width, out_height, &rgb)
            .map_err(|error| TranscodeError::Encode(error.to_string()))?;
        let access_unit = encoder
            .encode_frame(&frame420)
            .map_err(|error| TranscodeError::Encode(error.to_string()))?;

        writer
            .write_all(&access_unit)
            .map_err(|error| io_error(output_path, error))?;
        report_progress(progress, index, frames);
    }

    let tail = encoder.finish();
    writer
        .write_all(&tail)
        .map_err(|error| io_error(output_path, error))?;
    writer
        .flush()
        .map_err(|error| io_error(output_path, error))?;

    if frames == 0 {
        progress(1.0);
    }

    Ok(())
}

/// Compute letterbox/pillarbox geometry for a source frame.
pub(crate) fn target_geometry(
    source_width: u32,
    source_height: u32,
    par: (u32, u32),
    output_height: u32,
    target: Target,
) -> Option<Geometry> {
    if source_width == 0 || source_height == 0 || output_height == 0 {
        return None;
    }

    let par_num = if par.0 == 0 { 1 } else { par.0 };
    let par_den = if par.1 == 0 { 1 } else { par.1 };

    let aspect_num = u128::from(source_width).checked_mul(u128::from(par_num))?;
    let aspect_den = u128::from(source_height).checked_mul(u128::from(par_den))?;
    if aspect_num == 0 || aspect_den == 0 {
        return None;
    }

    let left = aspect_num.checked_mul(target.den)?;
    let right = target.num.checked_mul(aspect_den)?;

    let (width, height) = if left == right {
        (OUTPUT_WIDTH, output_height)
    } else if left > right {
        let height = scaled_height(output_height, aspect_num, aspect_den, target)?;
        (OUTPUT_WIDTH, height)
    } else {
        let width = scaled_width(output_height, aspect_num, aspect_den, target)?;
        (width, output_height)
    };

    let width = width.clamp(1, OUTPUT_WIDTH.max(1));
    let height = height.clamp(1, output_height.max(1));
    let x = OUTPUT_WIDTH.saturating_sub(width) / 2;
    let y = output_height.saturating_sub(height) / 2;

    Some(Geometry {
        x,
        y,
        width,
        height,
    })
}

fn scaled_height(
    output_height: u32,
    aspect_num: u128,
    aspect_den: u128,
    target: Target,
) -> Option<u32> {
    let numerator = u128::from(output_height)
        .checked_mul(target.num)?
        .checked_mul(aspect_den)?;
    let denominator = target.den.checked_mul(aspect_num)?;
    if denominator == 0 {
        return Some(output_height);
    }

    Some(clamp_u128(numerator / denominator, output_height))
}

fn scaled_width(
    output_height: u32,
    aspect_num: u128,
    aspect_den: u128,
    target: Target,
) -> Option<u32> {
    let numerator = u128::from(output_height)
        .checked_mul(aspect_num)?
        .checked_mul(target.den)?;
    let denominator = aspect_den.checked_mul(target.num)?;
    if denominator == 0 {
        return Some(OUTPUT_WIDTH);
    }

    Some(clamp_u128(numerator / denominator, OUTPUT_WIDTH))
}

fn clamp_u128(value: u128, max: u32) -> u32 {
    if value > u128::from(max) {
        max
    } else {
        u32::try_from(value).unwrap_or(max)
    }
}

fn output_height(standard: Standard) -> u32 {
    if standard == Standard::Ntsc { 480 } else { 576 }
}

fn frame_rate(standard: Standard) -> FrameRate {
    if standard == Standard::Ntsc {
        FrameRate::FPS_29_97
    } else {
        FrameRate::FPS_25
    }
}

fn encoder_config(options: &crate::TranscodeOptions, max_bitrate: u32) -> EncoderConfig {
    EncoderConfig {
        standard: options.standard,
        gop_size: if options.standard == Standard::Ntsc {
            15
        } else {
            12
        },
        bitrate: options.video_bitrate,
        max_bitrate,
        wide: options.wide,
    }
}

fn frame_index_to_i64(index: u64) -> Result<i64, TranscodeError> {
    i64::try_from(index)
        .map_err(|_| TranscodeError::Unsupported("frame index too large".to_string()))
}

fn report_progress(progress: &mut dyn FnMut(f32), index: u64, frames: u64) {
    let done = index.checked_add(1).unwrap_or(frames);
    let fraction = if frames == 0 {
        1.0
    } else {
        (done as f64 / frames as f64) as f32
    };

    progress(fraction.clamp(0.0, 1.0));
}

fn render_source_frame(
    frame: &VideoFrame,
    output_height: u32,
    target: Target,
) -> Result<Vec<u8>, TranscodeError> {
    let geometry = target_geometry(frame.width, frame.height, frame.par, output_height, target)
        .ok_or_else(|| TranscodeError::Unsupported("invalid frame geometry".to_string()))?;
    let rgba = frame.to_rgba8();

    render_rgb(&rgba, frame.width, frame.height, geometry, output_height)
        .ok_or_else(|| TranscodeError::Unsupported("failed to render frame".to_string()))
}

fn render_rgb(
    rgba: &[u8],
    source_width: u32,
    source_height: u32,
    geometry: Geometry,
    output_height: u32,
) -> Option<Vec<u8>> {
    let output_width = OUTPUT_WIDTH as usize;
    let height = output_height as usize;
    let size = output_width.checked_mul(height)?.checked_mul(3)?;
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

    let scaled = u64::from(offset)
        .checked_mul(u64::from(max_source))
        .and_then(|value| value.checked_mul(FIXED_POINT_ONE))
        .unwrap_or(0);

    scaled / u64::from(denom)
}

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
        .ok()?
        .min(width.saturating_sub(1));
    let y0 = usize::try_from(sy >> FIXED_POINT_SHIFT)
        .ok()?
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

fn composite_pixel(pixel: [u8; 4]) -> [u8; 3] {
    let [r, g, b, a] = pixel;
    [
        composite_channel(r, a),
        composite_channel(g, a),
        composite_channel(b, a),
    ]
}

fn composite_channel(color: u8, alpha: u8) -> u8 {
    let value = (u32::from(color) * u32::from(alpha) + 127) / 255;
    u8::try_from(value).unwrap_or(255)
}

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

fn interpolate_channel(c00: u8, c01: u8, c10: u8, c11: u8, fx: u32, fy: u32) -> u8 {
    let top =
        (u32::from(c00) * (FIXED_POINT_ONE as u32 - fx) + u32::from(c01) * fx) >> FIXED_POINT_SHIFT;
    let bottom =
        (u32::from(c10) * (FIXED_POINT_ONE as u32 - fx) + u32::from(c11) * fx) >> FIXED_POINT_SHIFT;
    let value = (top * (FIXED_POINT_ONE as u32 - fy) + bottom * fy) >> FIXED_POINT_SHIFT;

    u8::try_from(value).unwrap_or(255)
}

fn write_rgb(rgb: &mut [u8], output_width: usize, y: u32, x: u32, pixel: [u8; 3]) -> Option<()> {
    let row = usize::try_from(y).ok()?.checked_mul(output_width)?;
    let col = usize::try_from(x).ok()?;
    let index = row.checked_add(col)?.checked_mul(3)?;
    let end = index.checked_add(3)?;
    let slot = rgb.get_mut(index..end)?;
    slot.copy_from_slice(&pixel);
    Some(())
}

/// Calls `f(index, rgb)` for the first `frames` output frames of `source` (RGB24, 720 × 480 / 576, 4:3 letterboxed), at the DVD frame rate.
/// The frame at `time` as letterboxed RGB24 at the NTSC DVD frame size (720 x 480, 4:3).
pub(crate) fn rgb_frame_at(source: &Source, time: Tick) -> Result<Vec<u8>, TranscodeError> {
    let frame = source.video_frame(time)?;
    render_source_frame(&frame, output_height(Standard::Ntsc), FOUR_THREE)
}

pub(crate) fn for_each_rgb_frame(
    source: &Source,
    standard: Standard,
    frames: u64,
    f: &mut dyn FnMut(u64, &[u8]) -> Result<(), String>,
) -> Result<(), TranscodeError> {
    let output_height = output_height(standard);
    let rate = frame_rate(standard);
    for index in 0..frames {
        let time = rate.tick_of(frame_index_to_i64(index)?);
        let frame = source.video_frame(time)?;
        let rgb = render_source_frame(&frame, output_height, FOUR_THREE)?;
        f(index, &rgb).map_err(TranscodeError::Encode)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{FOUR_THREE, SIXTEEN_NINE, target_geometry};

    #[test]
    fn geometry_16_9_ntsc() -> Result<(), String> {
        let geometry =
            target_geometry(1920, 1080, (1, 1), 480, FOUR_THREE).ok_or("missing geometry")?;
        assert_eq!(geometry.width, 720);
        assert_eq!(geometry.height, 360);
        assert_eq!(geometry.x, 0);
        assert_eq!(geometry.y, 60);
        Ok(())
    }

    #[test]
    fn geometry_16_9_pal() -> Result<(), String> {
        let geometry =
            target_geometry(1920, 1080, (1, 1), 576, FOUR_THREE).ok_or("missing geometry")?;
        assert_eq!(geometry.width, 720);
        assert_eq!(geometry.height, 432);
        assert_eq!(geometry.x, 0);
        assert_eq!(geometry.y, 72);
        Ok(())
    }

    #[test]
    fn geometry_16_9_into_wide_frame_fills_it() -> Result<(), String> {
        let g = target_geometry(1920, 1080, (1, 1), 480, SIXTEEN_NINE).ok_or("missing geometry")?;
        assert_eq!((g.x, g.y, g.width, g.height), (0, 0, 720, 480));
        Ok(())
    }

    #[test]
    fn geometry_4_3_full() -> Result<(), String> {
        let geometry =
            target_geometry(640, 480, (1, 1), 480, FOUR_THREE).ok_or("missing geometry")?;
        assert_eq!(geometry.width, 720);
        assert_eq!(geometry.height, 480);
        assert_eq!(geometry.x, 0);
        assert_eq!(geometry.y, 0);
        Ok(())
    }

    #[test]
    fn geometry_9_16_pillarbox() -> Result<(), String> {
        let geometry = target_geometry(9, 16, (1, 1), 480, FOUR_THREE).ok_or("missing geometry")?;
        assert_eq!(geometry.height, 480);
        assert!((i64::from(geometry.width) - 202).abs() <= 1);
        assert_eq!(geometry.x, (720 - geometry.width) / 2);
        assert_eq!(geometry.y, 0);
        Ok(())
    }
}
