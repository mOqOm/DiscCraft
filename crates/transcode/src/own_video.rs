//! Video formats DiscCraft decodes itself besides DV and MPEG (M6.561): what the media readers find in a file but have
//! no decoder for. PNG frames (QuickTime `png `, AVI and Matroska `MPNG`), each frame a whole PNG image decoded with
//! the `png` crate; M6.562: FFV1 (`FFV1`, Matroska `V_FFV1`) with disccraft-ffv1; M6.563: H.263 (QuickTime `h263`,
//! 3GP `s263`, AVI `H263`) with disccraft-h263. M6.567, with craft-libs' craft-video (its public API): Hap (`Hap1`,
//! `Hap5`, `HapY`, `HapM`, `HapA`; decoded to RGBA in software), uncompressed 10-bit RGB (`r210`, `R10k`) and Theora
//! (Matroska `V_THEORA`, and an Ogg file's Theora stream through [`crate::own_source`]).
//!
//! In a MOV / MP4 / 3GP file the media reader asks the registered decoder factories ([`register`]) for each video
//! track's sample entry; in an AVI or a Matroska file it does not, so [`crate::own_source`] reads those frames itself
//! and decodes them with [`decoder`].

use std::sync::{Arc, Once};

use craft_media::isobmff::{CodecConfig, SampleEntry};
use filmcraft_codecs::video::{DecodedFrame, VideoDecoder};
use filmcraft_codecs::{CodecError, Result as CodecResult};
use filmcraft_color::{ColorInfo, Matrix, Primaries, Range, Transfer};
use filmcraft_frame::{Chroma, PixelData, VideoFrame};
use filmcraft_media::VideoStreamInfo;
use filmcraft_time::{FrameRate, Tick};

/// The most pixels a frame of these formats may have (8192 x 4320): a hostile header asks for no more memory.
const MOST_PIXELS: u64 = 8192 * 4320;

/// A video format DiscCraft decodes itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    /// Each frame a PNG image.
    Png,
    /// FFV1 (RFC 9043).
    Ffv1,
    /// H.263 (ITU-T H.263, the baseline).
    H263,
    /// Hap (Vidvox): GPU texture frames.
    Hap,
    /// Uncompressed 10-bit RGB in 32-bit words, rows padded to 256 bytes (`r210`).
    R210,
    /// Uncompressed 10-bit RGB in 32-bit words, rows not padded (AJA `R10k`).
    R10k,
    /// Theora (Xiph.Org).
    Theora,
}

impl Format {
    /// The format a four-character code names (a QuickTime sample entry, an AVI stream's compression or handler, a
    /// Matroska track's Video for Windows compression), in any case.
    pub(crate) fn of_code(code: &[u8; 4]) -> Option<Self> {
        match &crate::avi::upper(code) {
            b"PNG " | b"MPNG" => Some(Self::Png),
            b"FFV1" => Some(Self::Ffv1),
            b"H263" | b"S263" => Some(Self::H263),
            b"HAP1" | b"HAP5" | b"HAPY" | b"HAPM" | b"HAPA" => Some(Self::Hap),
            b"R210" => Some(Self::R210),
            b"R10K" => Some(Self::R10k),
            _ => None,
        }
    }

    /// The format a Matroska track's codec id names: a native id, or Video for Windows with its compression code
    /// (`compression`, from the BITMAPINFOHEADER of the track's private data).
    pub(crate) fn of_matroska(codec_id: &str, compression: Option<[u8; 4]>) -> Option<Self> {
        match codec_id {
            "V_MS/VFW/FOURCC" => compression.and_then(|code| Self::of_code(&code)),
            "V_FFV1" => Some(Self::Ffv1),
            "V_THEORA" => Some(Self::Theora),
            _ => None,
        }
    }

    /// The format's name, as the media information shows it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Ffv1 => "FFV1",
            Self::H263 => "H.263",
            Self::Hap => "Hap",
            Self::R210 => "r210",
            Self::R10k => "R10k",
            Self::Theora => "Theora",
        }
    }

    /// Whether every frame is coded on its own (any frame decodes without the ones before it). An FFV1 frame that is
    /// not a key frame carries on the adaptive states of the frames before it.
    pub(crate) fn intra_only(self) -> bool {
        match self {
            Self::Png | Self::Hap | Self::R210 | Self::R10k => true,
            Self::Ffv1 | Self::H263 | Self::Theora => false,
        }
    }
}

/// Whether `frame` (coded in `format`) is a key frame: one that decodes without the frames before it.
pub(crate) fn is_key(format: Format, frame: &[u8]) -> bool {
    match format {
        Format::Png | Format::Hap | Format::R210 | Format::R10k => true,
        Format::Ffv1 => disccraft_ffv1::is_keyframe(frame),
        Format::H263 => disccraft_h263::is_intra(frame),
        // A data packet (first bit 0) of an intra frame (second bit 0), Theora §7.1.
        Format::Theora => frame.first().is_some_and(|byte| byte & 0xC0 == 0),
    }
}

/// A decoder of one of these formats at work: one coded frame in, its picture out.
pub(crate) trait FrameDecoder: Send {
    /// The picture of `frame`, shown at `pts`.
    ///
    /// # Errors
    ///
    /// When the frame cannot be decoded.
    fn decode(&mut self, frame: &[u8], pts: Tick) -> Result<VideoFrame, String>;
}

/// A decoder of `format`, with the codec's own setup data `extra` (empty when the container has none) and the
/// container's picture size (FFV1 takes it from there).
///
/// # Errors
///
/// When the setup data is not what the format needs.
pub(crate) fn decoder(
    format: Format,
    extra: &[u8],
    (width, height): (u32, u32),
) -> Result<Box<dyn FrameDecoder>, String> {
    match format {
        Format::Png => Ok(Box::new(PngFrames)),
        Format::Ffv1 => {
            disccraft_ffv1::Decoder::new(width, height, (!extra.is_empty()).then_some(extra))
                .map(|decoder| Box::new(Ffv1Frames(decoder)) as Box<dyn FrameDecoder>)
                .map_err(|error| error.to_string())
        }
        Format::H263 => Ok(Box::new(H263Frames(disccraft_h263::Decoder::new()))),
        Format::Hap => {
            check_size(width, height)?;
            craft_video::hap::Decoder::new(width, height)
                .map(|decoder| Box::new(HapFrames(decoder)) as Box<dyn FrameDecoder>)
                .map_err(|error| format!("Hap: {error}"))
        }
        Format::R210 | Format::R10k => {
            check_size(width, height)?;
            let layout = if format == Format::R210 {
                craft_video::rawvideo::Layout::R210
            } else {
                craft_video::rawvideo::Layout::R10k
            };
            Ok(Box::new(RgbFrames {
                layout,
                width,
                height,
            }))
        }
        Format::Theora => {
            let [identification, comment, setup] = xiph_unlace(extra)
                .ok_or_else(|| "Theora: the stream's three headers are missing".to_string())?;
            craft_video::theora::Decoder::from_headers(&identification, &comment, &setup)
                .map(|decoder| Box::new(TheoraFrames(decoder)) as Box<dyn FrameDecoder>)
                .map_err(|error| format!("Theora: {error}"))
        }
    }
}

/// A frame size the container gives, within [`MOST_PIXELS`].
fn check_size(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MOST_PIXELS {
        return Err(format!("{width} x {height} is not a frame size"));
    }
    Ok(())
}

/// Codec headers in Xiph lacing (a Matroska track's private data for Theora and Vorbis): the count less one, the
/// sizes of all but the last as runs of 255s and a remainder, then the headers one after another.
pub(crate) fn xiph_lace(headers: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![u8::try_from(headers.len().saturating_sub(1)).unwrap_or(u8::MAX)];
    for header in headers.iter().take(headers.len().saturating_sub(1)) {
        let mut size = header.len();
        while size >= 255 {
            out.push(255);
            size -= 255;
        }
        out.push(u8::try_from(size).unwrap_or(0));
    }
    for header in headers {
        out.extend_from_slice(header);
    }
    out
}

/// The three headers of a Theora stream from their Xiph lacing; `None` when the data is not three laced headers.
fn xiph_unlace(data: &[u8]) -> Option<[Vec<u8>; 3]> {
    if data.first() != Some(&2) {
        return None;
    }
    let mut at = 1usize;
    let mut sizes = [0usize; 2];
    for size in &mut sizes {
        loop {
            let byte = *data.get(at)?;
            at += 1;
            *size += usize::from(byte);
            if byte < 255 {
                break;
            }
        }
    }
    let first = data.get(at..at.checked_add(sizes[0])?)?;
    let second_start = at + sizes[0];
    let second = data.get(second_start..second_start.checked_add(sizes[1])?)?;
    let third = data.get(second_start + sizes[1]..)?;
    Some([first.to_vec(), second.to_vec(), third.to_vec()])
}

/// Hap frames, decoded to straight-alpha RGBA.
struct HapFrames(craft_video::hap::Decoder);

impl FrameDecoder for HapFrames {
    fn decode(&mut self, frame: &[u8], pts: Tick) -> Result<VideoFrame, String> {
        let image = self
            .0
            .decode_rgba(frame)
            .map_err(|error| format!("Hap frame: {error}"))?;
        Ok(VideoFrame {
            width: image.width,
            height: image.height,
            data: PixelData::Rgba8(Arc::new(image.rgba)),
            color: ColorInfo::SRGB_FULL,
            par: (1, 1),
            pts,
        })
    }
}

/// Uncompressed 10-bit RGB frames, cut to 8 bits a channel (the pictures are 8-bit from here on).
struct RgbFrames {
    layout: craft_video::rawvideo::Layout,
    width: u32,
    height: u32,
}

impl FrameDecoder for RgbFrames {
    fn decode(&mut self, frame: &[u8], pts: Tick) -> Result<VideoFrame, String> {
        let decoded = craft_video::rawvideo::decode(self.layout, self.width, self.height, frame)
            .map_err(|error| format!("RGB frame: {error}"))?;
        let shift = u32::from(decoded.bit_depth.saturating_sub(8));
        let [r, g, b] = &decoded.planes;
        let down = |value: u16| u8::try_from(value >> shift).unwrap_or(u8::MAX);
        let rgba = r
            .iter()
            .zip(g)
            .zip(b)
            .flat_map(|((r, g), b)| [down(*r), down(*g), down(*b), 255])
            .collect();
        Ok(VideoFrame {
            width: decoded.width,
            height: decoded.height,
            data: PixelData::Rgba8(Arc::new(rgba)),
            color: ColorInfo::SRGB_FULL,
            par: (1, 1),
            pts,
        })
    }
}

/// Theora frames: the picture region of each decoded frame.
struct TheoraFrames(craft_video::theora::Decoder);

impl FrameDecoder for TheoraFrames {
    fn decode(&mut self, frame: &[u8], pts: Tick) -> Result<VideoFrame, String> {
        let picture = self
            .0
            .decode_packet(frame)
            .map_err(|error| format!("Theora: {error}"))?;
        let info = &self.0.info;
        let par = if info.par_num == 0 || info.par_den == 0 {
            (1, 1)
        } else {
            (info.par_num, info.par_den)
        };
        theora_frame(&picture, par, pts)
    }
}

/// A Theora picture's display region as a media frame. The planes come as Theora codes them, bottom row first, and the
/// region is placed from the frame's lower left corner (Theora §2.2): its rows are taken from there and turned over.
fn theora_frame(
    picture: &craft_video::theora::Picture,
    par: (u32, u32),
    pts: Tick,
) -> Result<VideoFrame, String> {
    use craft_video::theora::PixelFormat;
    let (width, height) = (picture.pic_width as usize, picture.pic_height as usize);
    let (h_shift, v_shift, chroma) = match picture.pixel_format {
        PixelFormat::Yuv420 => (1, 1, Chroma::C420),
        PixelFormat::Yuv422 => (1, 0, Chroma::C422),
        PixelFormat::Yuv444 => (0, 0, Chroma::C444),
    };
    let left = picture.pic_x as usize;
    let bottom = picture.pic_y as usize;
    if left + width > picture.width as usize || bottom + height > picture.height as usize {
        return Err("Theora: the picture region is outside the frame".to_string());
    }
    // Rows `y` to `y + h` from the bottom, turned over (top row first).
    let crop = |plane: &[u8], stride: usize, (x, y, w, h): (usize, usize, usize, usize)| {
        let mut out = Vec::with_capacity(w * h);
        for row in (y..y + h).rev() {
            let start = row * stride + x;
            out.extend_from_slice(plane.get(start..start + w).ok_or("Theora: a short plane")?);
        }
        Ok::<_, String>(out)
    };
    let luma = crop(&picture.y, picture.y_stride, (left, bottom, width, height))?;
    let chroma_region = (
        left >> h_shift,
        bottom >> v_shift,
        (width + (1 << h_shift) - 1) >> h_shift,
        (height + (1 << v_shift) - 1) >> v_shift,
    );
    let cb = crop(&picture.u, picture.uv_stride, chroma_region)?;
    let cr = crop(&picture.v, picture.uv_stride, chroma_region)?;
    let color = theora_color(picture.color_space, height);
    Ok(VideoFrame {
        width: picture.pic_width,
        height: picture.pic_height,
        data: PixelData::Yuv8 {
            planes: [Arc::new(luma), Arc::new(cb), Arc::new(cr)],
            chroma,
            alpha: None,
        },
        color,
        par,
        pts,
    })
}

/// H.263 pictures.
struct H263Frames(disccraft_h263::Decoder);

impl FrameDecoder for H263Frames {
    fn decode(&mut self, frame: &[u8], pts: Tick) -> Result<VideoFrame, String> {
        let picture = self.0.decode(frame).map_err(|error| error.to_string())?;
        // ITU-R BT.601 (H.263 §4.1), the 625-line primaries for the formats of 288 and 576 lines.
        let lines_625 = picture.height % 288 == 0;
        Ok(VideoFrame {
            width: picture.width,
            height: picture.height,
            data: PixelData::Yuv8 {
                planes: [
                    Arc::new(picture.y),
                    Arc::new(picture.cb),
                    Arc::new(picture.cr),
                ],
                chroma: Chroma::C420,
                alpha: None,
            },
            color: ColorInfo {
                matrix: Matrix::Bt601,
                transfer: Transfer::Bt709,
                primaries: if lines_625 {
                    Primaries::Bt601_625
                } else {
                    Primaries::Bt601_525
                },
                range: Range::Limited,
            },
            par: picture.pixel_aspect,
            pts,
        })
    }
}

/// FFV1 frames.
struct Ffv1Frames(disccraft_ffv1::Decoder);

impl FrameDecoder for Ffv1Frames {
    fn decode(&mut self, frame: &[u8], pts: Tick) -> Result<VideoFrame, String> {
        let picture = self.0.decode(frame).map_err(|error| error.to_string())?;
        Ok(ffv1_frame(picture, pts))
    }
}

/// An FFV1 picture as a media frame: Y'CbCr as 8-bit or (above 8 bits) 16-bit planes, its chroma at 4:2:0, 4:2:2 or
/// 4:4:4 (other subsamplings widened to 4:4:4, grey given neutral chroma), transparency kept; RGB as 8-bit RGBA. The
/// stream does not say its colours: BT.601 up to 576 lines, BT.709 above, in the studio range (sRGB for RGB).
fn ffv1_frame(picture: disccraft_ffv1::Picture, pts: Tick) -> VideoFrame {
    let width = picture.width as usize;
    let height = picture.height as usize;
    let bits = picture.bits.clamp(8, 16);
    let par = picture.sample_aspect.unwrap_or((1, 1));
    let mut planes = picture.planes.into_iter();
    if picture.layout == disccraft_ffv1::Layout::Gbr {
        let (g, b, r) = (
            planes.next().unwrap_or_default(),
            planes.next().unwrap_or_default(),
            planes.next().unwrap_or_default(),
        );
        let alpha = if picture.alpha { planes.next() } else { None };
        let down = |value: u16| u8::try_from(value >> (bits - 8)).unwrap_or(u8::MAX);
        let mut rgba = Vec::with_capacity(width * height * 4);
        for at in 0..width * height {
            let sample = |plane: &[u16]| plane.get(at).copied().unwrap_or(0);
            rgba.extend_from_slice(&[
                down(sample(&r)),
                down(sample(&g)),
                down(sample(&b)),
                alpha.as_deref().map_or(255, |alpha| down(sample(alpha))),
            ]);
        }
        return VideoFrame {
            width: picture.width,
            height: picture.height,
            data: PixelData::Rgba8(Arc::new(rgba)),
            color: ColorInfo::SRGB_FULL,
            par,
            pts,
        };
    }
    let luma = planes.next().unwrap_or_default();
    let (shifts, chroma_planes) = match picture.layout {
        disccraft_ffv1::Layout::Yuv { h_shift, v_shift } => (
            (h_shift, v_shift),
            Some((
                planes.next().unwrap_or_default(),
                planes.next().unwrap_or_default(),
            )),
        ),
        _ => ((1, 1), None),
    };
    let alpha = if picture.alpha { planes.next() } else { None };
    let (chroma, (cw, ch)) = match shifts {
        (1, 1) => (Chroma::C420, (width.div_ceil(2), height.div_ceil(2))),
        (1, 0) => (Chroma::C422, (width.div_ceil(2), height)),
        _ => (Chroma::C444, (width, height)),
    };
    let neutral = 1u16 << (bits - 1);
    let (cb, cr) = match chroma_planes {
        // Kept as they are at 4:2:0, 4:2:2 and 4:4:4; widened (each sample repeated) to 4:4:4 otherwise.
        Some((cb, cr)) if matches!(shifts, (1, 1) | (1, 0) | (0, 0)) => (cb, cr),
        Some((cb, cr)) => {
            let (h, v) = shifts;
            let stride = width.div_ceil(1 << h);
            let widen = |plane: &[u16]| -> Vec<u16> {
                (0..width * height)
                    .map(|at| {
                        let (x, y) = (at % width, at / width);
                        plane
                            .get((y >> v) * stride + (x >> h))
                            .copied()
                            .unwrap_or(neutral)
                    })
                    .collect()
            };
            (widen(&cb), widen(&cr))
        }
        None => (vec![neutral; cw * ch], vec![neutral; cw * ch]),
    };
    let color = if height <= 576 {
        ColorInfo {
            matrix: Matrix::Bt601,
            transfer: Transfer::Bt709,
            primaries: if height == 576 {
                Primaries::Bt601_625
            } else {
                Primaries::Bt601_525
            },
            range: Range::Limited,
        }
    } else {
        ColorInfo::REC709
    };
    let data = if bits <= 8 {
        let narrow = |plane: Vec<u16>| {
            Arc::new(
                plane
                    .into_iter()
                    .map(|v| u8::try_from(v).unwrap_or(u8::MAX))
                    .collect::<Vec<u8>>(),
            )
        };
        PixelData::Yuv8 {
            planes: [narrow(luma), narrow(cb), narrow(cr)],
            chroma,
            alpha: alpha.map(narrow),
        }
    } else {
        PixelData::Yuv16 {
            planes: [Arc::new(luma), Arc::new(cb), Arc::new(cr)],
            chroma,
            bits,
            alpha: alpha.map(Arc::new),
        }
    };
    VideoFrame {
        width: picture.width,
        height: picture.height,
        data,
        color,
        par,
        pts,
    }
}

/// A Theora stream's colours: the primaries its header names (Rec. 470 M / BG, BT.601's 525 / 625), else BT.601 up to
/// 576 lines and BT.709 above; studio range.
fn theora_color(space: craft_video::theora::ColorSpace, height: usize) -> ColorInfo {
    use craft_video::theora::ColorSpace;
    let primaries = match space {
        ColorSpace::Rec470M => Primaries::Bt601_525,
        ColorSpace::Rec470Bg => Primaries::Bt601_625,
        ColorSpace::Undefined if height == 576 => Primaries::Bt601_625,
        ColorSpace::Undefined if height < 576 => Primaries::Bt601_525,
        ColorSpace::Undefined => return ColorInfo::REC709,
    };
    ColorInfo {
        matrix: Matrix::Bt601,
        transfer: Transfer::Bt709,
        primaries,
        range: Range::Limited,
    }
}

/// The video stream an Ogg file's Theora headers describe (the media reader shows none for it); `None` when they are
/// not three good Theora headers.
pub(crate) fn theora_stream(headers: &[Vec<u8>]) -> Option<VideoStreamInfo> {
    use craft_video::theora::PixelFormat;
    let [identification, comment, setup] = headers else {
        return None;
    };
    let info = craft_video::theora::Decoder::from_headers(identification, comment, setup)
        .ok()?
        .info;
    if info.fps_num == 0 || info.fps_den == 0 {
        return None;
    }
    check_size(info.pic_width, info.pic_height).ok()?;
    Some(VideoStreamInfo {
        width: info.pic_width,
        height: info.pic_height,
        frame_rate: FrameRate {
            num: i64::from(info.fps_num),
            den: i64::from(info.fps_den),
        },
        par: if info.par_num == 0 || info.par_den == 0 {
            (1, 1)
        } else {
            (info.par_num, info.par_den)
        },
        codec: Format::Theora.name().to_owned(),
        pixel_format: match info.pixel_format {
            PixelFormat::Yuv420 => "yuv420p",
            PixelFormat::Yuv422 => "yuv422p",
            PixelFormat::Yuv444 => "yuv444p",
        }
        .to_owned(),
        color: theora_color(info.color_space, info.pic_height as usize),
        has_alpha: false,
        bitrate: None,
        hdr: None,
        color_assumed: None,
    })
}

/// PNG frames: each one a whole image.
struct PngFrames;

impl FrameDecoder for PngFrames {
    fn decode(&mut self, frame: &[u8], pts: Tick) -> Result<VideoFrame, String> {
        let (width, height, rgba) = decode_png(frame)?;
        Ok(VideoFrame {
            width,
            height,
            data: PixelData::Rgba8(Arc::new(rgba)),
            color: ColorInfo::SRGB_FULL,
            par: (1, 1),
            pts,
        })
    }
}

/// A PNG image as straight-alpha RGBA, 8 bits a channel: palettes and grey expanded, 16-bit samples cut to their high
/// byte.
fn decode_png(data: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let failed = |error: png::DecodingError| format!("PNG frame: {error}");
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    decoder.set_limits(png::Limits {
        bytes: usize::try_from(MOST_PIXELS * 4 * 2).unwrap_or(usize::MAX),
    });
    let mut reader = decoder.read_info().map_err(failed)?;
    let (width, height) = {
        let info = reader.info();
        (info.width, info.height)
    };
    if u64::from(width) * u64::from(height) > MOST_PIXELS || width == 0 || height == 0 {
        return Err(format!("PNG frame: {width} x {height} is too large"));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| "PNG frame: too large".to_string())?;
    let mut buffer = vec![0u8; size];
    let output = reader.next_frame(&mut buffer).map_err(failed)?;
    let (color, _) = reader.output_color_type();
    let pixels = usize::try_from(u64::from(width) * u64::from(height)).unwrap_or(0);
    let line = output.line_size;
    let mut rgba = Vec::with_capacity(pixels * 4);
    let channels = match color {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("PNG frame: a palette left unexpanded".to_string()),
    };
    for row in 0..usize::try_from(height).unwrap_or(0) {
        let start = row * line;
        let Some(samples) =
            buffer.get(start..start + usize::try_from(width).unwrap_or(0) * channels)
        else {
            return Err("PNG frame: shorter than its size".to_string());
        };
        for pixel in samples.chunks_exact(channels) {
            match *pixel {
                [grey] => rgba.extend_from_slice(&[grey, grey, grey, 255]),
                [grey, alpha] => rgba.extend_from_slice(&[grey, grey, grey, alpha]),
                [red, green, blue] => rgba.extend_from_slice(&[red, green, blue, 255]),
                [red, green, blue, alpha] => rgba.extend_from_slice(&[red, green, blue, alpha]),
                _ => {}
            }
        }
    }
    Ok((width, height, rgba))
}

/// Registers the decoders of these formats with the media readers (once; later calls do nothing).
pub(crate) fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| filmcraft_codecs::register_video_decoder(factory));
}

/// A decoder for a MOV / MP4 / 3GP track whose sample entry is one of these formats.
fn factory(entry: &SampleEntry) -> Option<CodecResult<Box<dyn VideoDecoder>>> {
    let format = Format::of_code(&entry.format.0)?;
    let extra = match &entry.codec {
        CodecConfig::Unknown { raw, .. } => sample_entry_extra(raw),
        _ => Vec::new(),
    };
    let size = entry.video.as_ref().map_or((0, 0), |video| {
        (u32::from(video.width), u32::from(video.height))
    });
    Some(
        decoder(format, &extra, size)
            .map(|inner| Box::new(ForReader { inner, format }) as Box<dyn VideoDecoder>)
            .map_err(CodecError::Decode),
    )
}

/// The codec's setup data in a QuickTime visual sample entry's payload (`raw`, after its box header): the payload of
/// its `glbl` box, after the 78 bytes of the entry's own fields (empty when there is none).
pub(crate) fn sample_entry_extra(raw: &[u8]) -> Vec<u8> {
    let mut at = 78usize;
    while let Some(head) = raw.get(at..at + 8) {
        let size = head
            .get(..4)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .map_or(0, u32::from_be_bytes) as usize;
        if size < 8 {
            break;
        }
        if head.get(4..8) == Some(b"glbl") {
            return raw
                .get(at + 8..at + size)
                .map(<[u8]>::to_vec)
                .unwrap_or_default();
        }
        at = at.saturating_add(size);
    }
    Vec::new()
}

/// One of these decoders as the media readers take one.
struct ForReader {
    inner: Box<dyn FrameDecoder>,
    format: Format,
}

impl VideoDecoder for ForReader {
    fn decode(&mut self, sample: &[u8], pts: i64) -> CodecResult<Vec<DecodedFrame>> {
        let frame = self
            .inner
            .decode(sample, Tick(0))
            .map_err(CodecError::Decode)?;
        Ok(vec![DecodedFrame {
            pts,
            frame,
            draft: false,
        }])
    }

    fn flush(&mut self) -> Vec<DecodedFrame> {
        Vec::new()
    }

    fn reset(&mut self) {}

    fn name(&self) -> &str {
        self.format.name()
    }

    fn intra_only(&self) -> bool {
        self.format.intra_only()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::{Format, decode_png, sample_entry_extra};

    #[test]
    fn the_codes_name_the_formats_in_any_case() {
        assert_eq!(Format::of_code(b"png "), Some(Format::Png));
        assert_eq!(Format::of_code(b"MPNG"), Some(Format::Png));
        assert_eq!(Format::of_code(b"mpng"), Some(Format::Png));
        assert_eq!(Format::of_code(b"avc1"), None);
        assert_eq!(
            Format::of_matroska("V_MS/VFW/FOURCC", Some(*b"MPNG")),
            Some(Format::Png)
        );
        assert_eq!(Format::of_matroska("V_MPEG4/ISO/AVC", None), None);
        assert_eq!(Format::of_code(b"Hap1"), Some(Format::Hap));
        assert_eq!(Format::of_code(b"r210"), Some(Format::R210));
        assert_eq!(Format::of_code(b"R10k"), Some(Format::R10k));
        assert_eq!(Format::of_matroska("V_THEORA", None), Some(Format::Theora));
    }

    /// Three headers go through Xiph lacing and back, a size of 255 and more included; broken lacing is `None`.
    #[test]
    fn xiph_lacing_goes_both_ways() {
        let headers = vec![vec![1u8; 300], vec![2u8; 255], vec![3u8; 7]];
        let laced = super::xiph_lace(&headers);
        assert_eq!(&laced[..5], &[2, 255, 45, 255, 0]);
        assert_eq!(super::xiph_unlace(&laced).map(Vec::from), Some(headers));
        assert!(super::xiph_unlace(&laced[..100]).is_none());
        assert!(super::xiph_unlace(&[1, 2, 3]).is_none());
        assert!(super::xiph_unlace(&[]).is_none());
    }

    /// A `glbl` box after the entry's fields gives its payload; other boxes are passed over.
    #[test]
    fn the_setup_data_is_the_glbl_box() {
        let mut raw = vec![0u8; 78];
        raw.extend_from_slice(&[0, 0, 0, 10, b'f', b'i', b'e', b'l', 1, 0]);
        raw.extend_from_slice(&[0, 0, 0, 11, b'g', b'l', b'b', b'l', 7, 8, 9]);
        assert_eq!(sample_entry_extra(&raw), vec![7, 8, 9]);
        assert!(sample_entry_extra(&raw[..80]).is_empty());
    }

    /// A PNG frame comes out as RGBA; a damaged one is an error, not a panic.
    #[test]
    fn a_png_frame_decodes_to_rgba() {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 2, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[255, 0, 0, 0, 0, 255]).unwrap();
        }
        let (width, height, rgba) = decode_png(&encoded).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(rgba, vec![255, 0, 0, 255, 0, 0, 255, 255]);
        assert!(decode_png(&encoded[..encoded.len() / 2]).is_err());
        assert!(decode_png(b"not a png").is_err());
    }
}
