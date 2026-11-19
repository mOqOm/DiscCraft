//! Render slideshows to DVD-compliant MPEG-2 video and LPCM WAV audio.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use disccraft_mpeg2venc::encoder::{Encoder, EncoderConfig};
use disccraft_mpeg2venc::{Standard, rgb_to_frame420};
use filmcraft_frame::VideoFrame;
use filmcraft_time::Tick;

use crate::audio;
use crate::source::Source;
use crate::{TranscodeError, TranscodeOptions, TranscodeOutput, io_error};

/// How a slide's picture fills the frame (Encore's slide "Scale").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlideFit {
    /// Scaled to fit inside the frame, the rest black (letterbox / pillarbox).
    #[default]
    ScaleAndMatte,
    /// Scaled to cover the frame, the overflow cut off.
    ScaleAndCrop,
    /// One image pixel per frame pixel, centred; larger pictures are cut, smaller ones get black around.
    NoScale,
}

/// One slideshow slide input.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideInput {
    /// Path to a still image media file.
    pub image: PathBuf,
    /// Display duration in seconds.
    pub seconds: f64,
    /// How the picture fills the frame.
    pub fit: SlideFit,
    /// The transition into this slide when it differs from the show's (ignored on the first slide).
    pub transition: Option<Transition>,
    /// Pan and zoom while the slide is up (Encore's slide effect); `None` holds the picture still.
    pub motion: Option<Motion>,
    /// The picture's alpha channel is ignored (Encore's Interpret Footage ▸ Ignore Alpha Channel): every pixel is
    /// opaque, not composited over black.
    pub opaque: bool,
}

/// A slide's pan and zoom: the picture is scaled by up to [`MOTION_SCALE`] and travels across the room that
/// leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Motion {
    /// The way the picture travels across the frame: -1, 0 or 1 horizontally (left / right) and vertically
    /// (up / down).
    pub pan: (i8, i8),
    /// 1 grows the picture (zoom in), -1 shrinks it back to its size (zoom out), 0 keeps its size.
    pub zoom: i8,
}

/// How much a slide's picture is enlarged at the far end of a zoom (and all through a pan without zoom, which
/// needs room to travel in).
const MOTION_SCALE: f32 = 1.12;

impl Motion {
    /// The scale of the picture and where its centre sits (in frame widths and heights from the frame's centre)
    /// at `t` from 0 (the slide's first frame) to 1 (its last). The frame shows the part of the slide (as fitted
    /// into the frame) of `1 / scale` its size around that point.
    #[must_use]
    pub fn at(self, t: f32) -> (f32, f32, f32) {
        let t = t.clamp(0.0, 1.0);
        let scale = match self.zoom.signum() {
            1 => 1.0 + (MOTION_SCALE - 1.0) * t,
            -1 => MOTION_SCALE - (MOTION_SCALE - 1.0) * t,
            // No zoom: enlarged all the way when the picture pans, as it is otherwise.
            _ if self.pan != (0, 0) => MOTION_SCALE,
            _ => 1.0,
        };
        // The part of the picture the frame cannot show, as a share of the frame: the room to travel in.
        let slack = 1.0 - 1.0 / scale;
        // From one side to the other over the slide. The picture moving left means the view moving right.
        let along = (2.0 * t - 1.0) * slack / 2.0;
        (
            scale,
            f32::from(self.pan.0.signum()) * -along,
            f32::from(self.pan.1.signum()) * -along,
        )
    }

    /// `rgb` (a frame of `width` x `height`) as the slide shows it at `t`.
    fn frame(self, rgb: &[u8], (width, height): (u32, u32), t: f32) -> Vec<u8> {
        let (scale, centre_x, centre_y) = self.at(t);
        let (w, h) = (width as usize, height as usize);
        if scale <= 1.0 && centre_x == 0.0 && centre_y == 0.0
            || rgb.len() < w * h * 3
            || w == 0
            || h == 0
        {
            return rgb.to_vec();
        }
        let (wf, hf) = (width as f32, height as f32);
        // The source position of the frame's centre.
        let (cx, cy) = (wf / 2.0 + centre_x * wf, hf / 2.0 + centre_y * hf);
        let mut out = vec![0u8; w * h * 3];
        let sample = |x: usize, y: usize, channel: usize| -> f32 {
            rgb.get((y * w + x) * 3 + channel)
                .copied()
                .map_or(0.0, f32::from)
        };
        for (y, row) in out.chunks_exact_mut(w * 3).enumerate() {
            let sy = (cy + (y as f32 + 0.5 - hf / 2.0) / scale - 0.5).clamp(0.0, hf - 1.0);
            let y0 = sy.floor() as usize;
            let y1 = (y0 + 1).min(h - 1);
            let fy = sy - y0 as f32;
            for (x, pixel) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let sx = (cx + (x as f32 + 0.5 - wf / 2.0) / scale - 0.5).clamp(0.0, wf - 1.0);
                let x0 = sx.floor() as usize;
                let x1 = (x0 + 1).min(w - 1);
                let fx = sx - x0 as f32;
                for (channel, value) in pixel.iter_mut().enumerate() {
                    let top = sample(x0, y0, channel) * (1.0 - fx) + sample(x1, y0, channel) * fx;
                    let bottom =
                        sample(x0, y1, channel) * (1.0 - fx) + sample(x1, y1, channel) * fx;
                    // `as` saturates; the mix of bytes stays in their range.
                    *value = (top * (1.0 - fy) + bottom * fy).round() as u8;
                }
            }
        }
        out
    }
}

/// Where a picture lands in the output frame; it may stick out (negative x / y, or a size past the frame) and is
/// then cut to the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Placement {
    x: i64,
    y: i64,
    width: u32,
    height: u32,
}

/// A transition between consecutive slides (Encore's slideshow transitions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlideTransition {
    /// A hard cut.
    #[default]
    None,
    /// The next slide fades in over the previous one.
    CrossDissolve,
    /// The previous slide fades to black, then the next fades in.
    DipToBlack,
    /// The next slide wipes in from the left.
    Wipe,
}

/// The transition at every cut of a slideshow and its length. The transition is centred on the cut: half of it
/// takes the end of the previous slide, half the start of the next, so the slides keep their in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
    /// Which transition.
    pub kind: SlideTransition,
    /// Its length in seconds (0 or `None` means a hard cut).
    pub seconds: f64,
}

impl Transition {
    /// A hard cut.
    pub const NONE: Transition = Transition {
        kind: SlideTransition::None,
        seconds: 0.0,
    };
}

/// The longest transition, in seconds (longer requests are cut to this).
const MAX_TRANSITION_SECONDS: f64 = 10.0;

const OUTPUT_WIDTH: u32 = 720;
const FIXED_POINT_SHIFT: u32 = 16;
const FIXED_POINT_ONE: u64 = 1 << FIXED_POINT_SHIFT;

#[derive(Clone, Copy)]
struct Target {
    num: u128,
    den: u128,
}

const FOUR_THREE: Target = Target { num: 4, den: 3 };
const SIXTEEN_NINE: Target = Target { num: 16, den: 9 };

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Geometry {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Render a slideshow into one MPEG-2 video elementary stream and one WAV file.
///
/// Each slide is opened as a one-frame video source, letterboxed or pillarboxed to
/// the DVD frame, converted once to 4:2:0, and encoded for `round(seconds * fps)`
/// frames. A single encoder is used for the whole show so GOPs continue across
/// slides.
///
/// The audio clips play one after another, transcoded to a 48 kHz stereo 16-bit WAV cut to the show length;
/// with `loop_audio` they start over until the show ends, otherwise silence pads the rest. Without clips a
/// silent WAV of the show's length is written.
#[allow(clippy::too_many_arguments)]
pub fn render_slideshow(
    slides: &[SlideInput],
    transition: Transition,
    audio: &[PathBuf],
    loop_audio: bool,
    video_out: &Path,
    audio_out: &Path,
    options: &TranscodeOptions,
    progress: &mut dyn FnMut(f32),
) -> Result<TranscodeOutput, TranscodeError> {
    if options.video_bitrate == 0 {
        return Err(unsupported("video bitrate must be non-zero"));
    }

    let max_bitrate = if options.max_bitrate == 0 {
        options.video_bitrate
    } else {
        options.max_bitrate.max(options.video_bitrate)
    };

    let (frame_counts, total_frames, total_seconds) =
        validate_slides(slides, fps(options.standard))?;

    let frames = encode_slideshow_video(
        slides,
        &frame_counts,
        transition,
        video_out,
        options,
        max_bitrate,
        total_frames,
        progress,
    )?;

    write_slideshow_audio(audio, loop_audio, audio_out, total_seconds)?;

    Ok(TranscodeOutput {
        video: video_out.to_path_buf(),
        audio: Some(audio_out.to_path_buf()),
        frames,
    })
}

/// A slideshow's sound alone, as [`render_slideshow`] writes it beside the pictures: the clips one after another
/// (or silence) as a 48 kHz stereo 16-bit WAV of the show's length. For a disc whose pictures are made in another
/// format ([`render_slideshow_hd`]).
///
/// # Errors
///
/// When the slides' lengths are out of range or a clip cannot be read.
pub fn render_slideshow_audio(
    slides: &[SlideInput],
    audio: &[PathBuf],
    loop_audio: bool,
    audio_out: &Path,
    standard: Standard,
) -> Result<(), TranscodeError> {
    let (_, _, total_seconds) = validate_slides(slides, fps(standard))?;
    write_slideshow_audio(audio, loop_audio, audio_out, total_seconds)
}

/// Render a slideshow's pictures for an HD Blu-ray disc: H.264 of `options`' size and rate on a 16:9 frame
/// (square pixels) to `es_out`, its access-unit index in `index_out` (as [`crate::hd::transcode_h264`] writes
/// them). The slides are placed, held and mixed as [`render_slideshow`] does; the audio is
/// [`render_slideshow_audio`]'s.
/// Returns the number of frames.
///
/// # Errors
///
/// When a slide cannot be read or the pictures cannot be encoded.
pub fn render_slideshow_hd(
    slides: &[SlideInput],
    transition: Transition,
    (es_out, index_out): (&Path, &Path),
    options: &crate::hd::HdOptions,
    progress: &mut dyn FnMut(f32),
) -> Result<u64, TranscodeError> {
    let rate = (u64::from(options.fps_num), u64::from(options.fps_den));
    let (frame_counts, total_frames, _) = validate_slides(slides, rate)?;
    let canvas = Canvas {
        width: options.width,
        height: options.height,
        target: SIXTEEN_NINE,
        fps: rate,
    };
    let mut encoder = crate::hd::H264Frames::new(options)?;
    let mut written = 0u64;
    for_each_frame(slides, &frame_counts, transition, canvas, &mut |rgb, _| {
        encoder.push_rgb(rgb)?;
        written = written.saturating_add(1);
        report_progress(progress, written, total_frames);
        crate::check_stop()
    })?;
    let (data, units) = encoder.finish()?;
    std::fs::write(es_out, &data).map_err(|error| io_error(es_out, error))?;
    crate::hd::write_h264_index(index_out, &units)?;
    progress(1.0);
    Ok(written)
}

fn validate_slides(
    slides: &[SlideInput],
    fps: (u64, u64),
) -> Result<(Vec<u64>, u64, f64), TranscodeError> {
    if slides.is_empty() {
        return Err(unsupported("slideshow has no slides"));
    }

    let mut frame_counts = Vec::with_capacity(slides.len());
    let mut total_frames = 0u64;
    let mut total_seconds = 0f64;

    for slide in slides {
        let frames = frames_for_seconds(slide.seconds, fps)
            .ok_or_else(|| unsupported("slide duration out of range"))?;

        if frames == 0 {
            return Err(unsupported("slide duration too short"));
        }

        frame_counts.push(frames);
        total_frames = total_frames
            .checked_add(frames)
            .ok_or_else(|| unsupported("slideshow frame count overflow"))?;

        total_seconds += slide.seconds;
        if !total_seconds.is_finite() || total_seconds < 0.0 {
            return Err(unsupported("slideshow duration out of range"));
        }
    }

    if total_frames == 0 {
        return Err(unsupported("slideshow has no output frames"));
    }

    Ok((frame_counts, total_frames, total_seconds))
}

/// The frames a slideshow is drawn on: their size, the shape they are shown at, and their rate.
#[derive(Clone, Copy)]
struct Canvas {
    width: u32,
    height: u32,
    target: Target,
    fps: (u64, u64),
}

#[allow(clippy::too_many_arguments)]
fn encode_slideshow_video(
    slides: &[SlideInput],
    frame_counts: &[u64],
    transition: Transition,
    video_out: &Path,
    options: &TranscodeOptions,
    max_bitrate: u32,
    total_frames: u64,
    progress: &mut dyn FnMut(f32),
) -> Result<u64, TranscodeError> {
    let file = File::create(video_out).map_err(|error| io_error(video_out, error))?;
    let mut writer = BufWriter::new(file);
    let mut encoder = Encoder::new(encoder_config(options, max_bitrate))
        .map_err(|error| TranscodeError::Encode(error.to_string()))?;

    let standard = options.standard;
    let canvas = Canvas {
        width: OUTPUT_WIDTH,
        height: output_height(standard),
        target: if options.wide {
            SIXTEEN_NINE
        } else {
            FOUR_THREE
        },
        fps: fps(standard),
    };
    let out_width =
        u16::try_from(canvas.width).map_err(|_| unsupported("output width too large"))?;
    let out_height =
        u16::try_from(canvas.height).map_err(|_| unsupported("output height too large"))?;

    // A slide's own picture is converted once, not for every frame it is held.
    let mut held: Option<(usize, disccraft_mpeg2venc::Frame420)> = None;
    let mut written_frames = 0u64;
    for_each_frame(
        slides,
        frame_counts,
        transition,
        canvas,
        &mut |rgb, still| {
            let fresh;
            let frame420 = match (still, &held) {
                (Some(index), Some((at, frame))) if *at == index => frame,
                _ => {
                    let converted = rgb_to_frame420(out_width, out_height, rgb)
                        .map_err(|error| TranscodeError::Encode(error.to_string()))?;
                    match still {
                        Some(index) => {
                            held = Some((index, converted));
                            &held.as_ref().ok_or_else(|| unsupported("no picture"))?.1
                        }
                        None => {
                            fresh = converted;
                            &fresh
                        }
                    }
                }
            };
            let access_unit = encoder
                .encode_frame(frame420)
                .map_err(|error| TranscodeError::Encode(error.to_string()))?;
            writer
                .write_all(&access_unit)
                .map_err(|error| io_error(video_out, error))?;
            written_frames = written_frames
                .checked_add(1)
                .ok_or_else(|| unsupported("frame count overflow"))?;
            report_progress(progress, written_frames, total_frames);
            crate::check_stop()
        },
    )?;

    let tail = encoder.finish();
    writer
        .write_all(&tail)
        .map_err(|error| io_error(video_out, error))?;
    writer.flush().map_err(|error| io_error(video_out, error))?;

    progress(1.0);
    Ok(written_frames)
}

/// Calls `frame` with every picture of a slideshow on `canvas`, in order: each slide held for its frames, the
/// transitions mixed in at the cuts. `frame(rgb, still)` gets `still` = the slide's index when the picture is the
/// slide itself (the same picture as the frame before, unless the slide just started), `None` for a mixed one.
fn for_each_frame(
    slides: &[SlideInput],
    frame_counts: &[u64],
    transition: Transition,
    canvas: Canvas,
    frame: &mut dyn FnMut(&[u8], Option<usize>) -> Result<(), TranscodeError>,
) -> Result<(), TranscodeError> {
    // The transition into each slide (the show's unless the slide has its own), and half of it on each side of
    // the cut, never more than half a slide.
    let cuts: Vec<Transition> = slides
        .iter()
        .map(|slide| slide.transition.unwrap_or(transition))
        .collect();
    let wanted: Vec<u64> = cuts
        .iter()
        .map(|cut| transition_half_frames(*cut, canvas.fps))
        .collect();
    let halves = cut_halves(frame_counts, &wanted);
    let size = (canvas.width, canvas.height);

    // The previous slide's picture, kept while the cut into the current slide is still blending.
    let mut previous_rgb: Option<Vec<u8>> = None;
    let mut next_rgb: Option<Vec<u8>> = None;

    // How far through its frames a slide is (0 at the first, 1 at the last).
    let progress = |position: u64, frames: u64| -> f32 {
        if frames <= 1 {
            0.0
        } else {
            // Frame counts of a slide are small; the float holds them exactly.
            position as f32 / (frames - 1) as f32
        }
    };
    // A slide's picture at `t`: moved by its pan and zoom, or as it is.
    let shown = |slide: &SlideInput, rgb: &[u8], t: f32| -> Option<Vec<u8>> {
        slide.motion.map(|motion| motion.frame(rgb, size, t))
    };

    for (index, (slide, frames)) in slides.iter().zip(frame_counts).enumerate() {
        let rgb = match next_rgb.take() {
            Some(rgb) => rgb,
            None => slide_rgb(&slide.image, size, canvas.target, (slide.fit, slide.opaque))?,
        };
        let half_in = halves.get(index).copied().unwrap_or(0);
        let half_out = halves.get(index.saturating_add(1)).copied().unwrap_or(0);
        // The next slide as its first frame shows it, for the cut out of this one.
        let mut next_first: Option<Vec<u8>> = None;
        if half_out > 0
            && let Some(next) = slides.get(index.saturating_add(1))
        {
            let base = slide_rgb(&next.image, size, canvas.target, (next.fit, next.opaque))?;
            next_first = shown(next, &base, 0.0);
            next_rgb = Some(base);
        }

        for position in 0..*frames {
            let moved = shown(slide, &rgb, progress(position, *frames));
            let current: &[u8] = moved.as_deref().unwrap_or(&rgb);
            // Position within the transition at the cut before this slide (second half) or after it (first half).
            let blend = if position < half_in {
                previous_rgb.as_deref().map(|prev| {
                    (
                        prev,
                        current,
                        half_in.saturating_add(position),
                        half_in,
                        cuts.get(index).map_or(transition.kind, |cut| cut.kind),
                    )
                })
            } else if frames.saturating_sub(position) <= half_out {
                next_first.as_deref().or(next_rgb.as_deref()).map(|next| {
                    (
                        current,
                        next,
                        position.saturating_sub(frames.saturating_sub(half_out)),
                        half_out,
                        cuts.get(index.saturating_add(1))
                            .map_or(transition.kind, |cut| cut.kind),
                    )
                })
            } else {
                None
            };
            match blend {
                Some((from, to, at, half, kind)) => {
                    let alpha = transition_alpha(at, half);
                    let width = usize::try_from(canvas.width).unwrap_or(0);
                    let mixed = transition_frame(kind, from, to, alpha, width);
                    frame(&mixed, None)?;
                }
                // A moving slide is a new picture every frame; a still one is the same as the frame before.
                None if moved.is_some() => frame(current, None)?,
                None => frame(&rgb, Some(index))?,
            }
        }
        // The slide as its last frame showed it, for the cut into the next one.
        previous_rgb = Some(shown(slide, &rgb, 1.0).unwrap_or(rgb));
    }
    Ok(())
}

/// One slide as a letterboxed RGB frame. A vector drawing (SVG) is rendered at the output size so it stays sharp;
/// a PSD is composited by PhotoCraft; other images are decoded as one-frame video.
fn slide_rgb(
    path: &Path,
    output: (u32, u32),
    target: Target,
    (fit, opaque): (SlideFit, bool),
) -> Result<Vec<u8>, TranscodeError> {
    let (output_width, output_height) = output;
    let still = if disccraft_photo::is_svg(path) {
        // Render generously (twice the frame) and let the letterbox scale it down.
        Some(disccraft_photo::render_svg_fit(
            path,
            output_width.saturating_mul(2),
            output_height.saturating_mul(2),
        ))
    } else if disccraft_photo::is_document(path) {
        Some(disccraft_photo::load_image(path))
    } else {
        None
    };
    let chosen = crate::pixel_aspect::chosen_pixel_aspect(path);
    let Some(still) = still else {
        let source = Source::open(path)?;
        let frame = source.video_frame(Tick(0))?;
        // A pixel shape the project chose is the frame's already; else the file's, else Encore's for its size (a
        // DVD-sized still has SD pixels).
        let aspect = chosen.unwrap_or_else(|| {
            crate::pixel_aspect::still_own_aspect((frame.width, frame.height), frame.par)
        });
        let frame = if aspect == frame.par {
            frame
        } else {
            let mut shaped = (*frame).clone();
            shaped.par = aspect;
            std::sync::Arc::new(shaped)
        };
        return render_source_frame(&frame, output, target, (fit, opaque));
    };
    let mut image = still.map_err(|error| TranscodeError::Decode(error.to_string()))?;
    if opaque {
        make_opaque(&mut image.rgba);
    }
    // A document's pixels as chosen, else Encore's for its size; a drawing (rendered at the frame's size) square.
    let aspect = chosen.unwrap_or_else(|| {
        if disccraft_photo::is_svg(path) {
            (1, 1)
        } else {
            crate::pixel_aspect::still_default_aspect(image.width, image.height)
        }
    });
    let placement = placement(image.width, image.height, aspect, output, target, fit)
        .ok_or_else(|| unsupported("invalid image geometry"))?;
    render_rgb(&image.rgba, image.width, image.height, placement, output)
        .ok_or_else(|| unsupported("failed to render slide"))
}

/// Every pixel of RGBA `rgba` opaque (Interpret Footage ▸ Ignore Alpha Channel).
fn make_opaque(rgba: &mut [u8]) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
}

/// Frames on each side of a cut for `transition` (half its length), 0 for a hard cut.
fn transition_half_frames(transition: Transition, fps: (u64, u64)) -> u64 {
    if transition.kind == SlideTransition::None {
        return 0;
    }
    let seconds = if transition.seconds.is_finite() {
        transition.seconds.clamp(0.0, MAX_TRANSITION_SECONDS)
    } else {
        0.0
    };
    frames_for_seconds(seconds / 2.0, fps).unwrap_or(0)
}

/// For each slide, the frames at its start that belong to the cut before it (0 for the first slide): the wanted
/// half for that cut, cut to half of the shorter neighbour so a transition never swallows a slide.
fn cut_halves(frame_counts: &[u64], wanted: &[u64]) -> Vec<u64> {
    let mut halves = vec![0; frame_counts.len()];
    for index in 1..frame_counts.len() {
        let before = frame_counts
            .get(index.saturating_sub(1))
            .copied()
            .unwrap_or(0)
            / 2;
        let after = frame_counts.get(index).copied().unwrap_or(0) / 2;
        let half = wanted.get(index).copied().unwrap_or(0);
        if let Some(slot) = halves.get_mut(index) {
            *slot = half.min(before).min(after);
        }
    }
    halves
}

/// How far a transition has come at `position` of its `2 * half` frames (0 to 1, never exactly either).
fn transition_alpha(position: u64, half: u64) -> f32 {
    let total = half.saturating_mul(2).saturating_add(1).max(1);
    // Small integers: exact in f32.
    (position.saturating_add(1) as f32 / total as f32).clamp(0.0, 1.0)
}

/// One frame of a transition from `from` to `to` (RGB, `width` pixels per row), `alpha` of the way through.
fn transition_frame(
    kind: SlideTransition,
    from: &[u8],
    to: &[u8],
    alpha: f32,
    width: usize,
) -> Vec<u8> {
    match kind {
        SlideTransition::None => to.to_vec(),
        SlideTransition::CrossDissolve => blend_rgb(from, to, alpha),
        SlideTransition::DipToBlack => {
            let black = vec![0_u8; from.len()];
            if alpha < 0.5 {
                blend_rgb(from, &black, alpha * 2.0)
            } else {
                blend_rgb(&black, to, (alpha - 0.5) * 2.0)
            }
        }
        SlideTransition::Wipe => {
            let stride = width.saturating_mul(3).max(1);
            // `as` saturates; the edge is a column count.
            let edge = ((width as f32 * alpha).round() as usize)
                .min(width)
                .saturating_mul(3);
            from.iter()
                .zip(to)
                .enumerate()
                .map(|(i, (a, b))| if i % stride < edge { *b } else { *a })
                .collect()
        }
    }
}

/// `from` and `to` mixed, `alpha` of `to`.
fn blend_rgb(from: &[u8], to: &[u8], alpha: f32) -> Vec<u8> {
    let alpha = alpha.clamp(0.0, 1.0);
    from.iter()
        .zip(to)
        .map(|(a, b)| {
            // 0..=255 after rounding; `as` saturates.
            (f32::from(*a) * (1.0 - alpha) + f32::from(*b) * alpha).round() as u8
        })
        .collect()
}

fn write_slideshow_audio(
    audio: &[PathBuf],
    loop_audio: bool,
    audio_out: &Path,
    total_seconds: f64,
) -> Result<(), TranscodeError> {
    let sample_frames = crate::audio::sample_frames_from_seconds(total_seconds)
        .ok_or_else(|| unsupported("audio duration out of range"))?;
    if audio.is_empty() {
        let frames_usize = usize::try_from(sample_frames)
            .map_err(|_| TranscodeError::Unsupported("slideshow too long".to_string()))?;
        write_silent_wav(audio_out, frames_usize)?;
        return Ok(());
    }
    let sources = audio
        .iter()
        .map(|path| Source::open_audio(path))
        .collect::<Result<Vec<_>, _>>()?;
    let _ = crate::audio::write_audio_sequence(&sources, sample_frames, loop_audio, audio_out)?;
    Ok(())
}

fn frames_for_seconds(seconds: f64, fps: (u64, u64)) -> Option<u64> {
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }

    let (fps_num, fps_den) = fps;
    let value = seconds * fps_num as f64 / fps_den as f64;

    if !value.is_finite() || value < 0.0 {
        return None;
    }

    let rounded = value.round();
    if rounded > u64::MAX as f64 {
        return None;
    }

    Some(rounded as u64)
}

fn fps(standard: Standard) -> (u64, u64) {
    if standard == Standard::Ntsc {
        (30_000, 1_001)
    } else {
        (25, 1)
    }
}

fn output_height(standard: Standard) -> u32 {
    if standard == Standard::Ntsc { 480 } else { 576 }
}

fn encoder_config(options: &TranscodeOptions, max_bitrate: u32) -> EncoderConfig {
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
        format: disccraft_mpeg2venc::encoder::Format::Sd,
        pulldown: false,
    }
}

fn report_progress(progress: &mut dyn FnMut(f32), done: u64, total: u64) {
    let fraction = if total == 0 {
        1.0
    } else {
        (done as f64 / total as f64) as f32
    };

    progress(fraction.clamp(0.0, 1.0));
}

fn unsupported(message: &str) -> TranscodeError {
    TranscodeError::Unsupported(message.to_string())
}

fn render_source_frame(
    frame: &VideoFrame,
    output: (u32, u32),
    target: Target,
    (fit, opaque): (SlideFit, bool),
) -> Result<Vec<u8>, TranscodeError> {
    let placement = placement(frame.width, frame.height, frame.par, output, target, fit)
        .ok_or_else(|| unsupported("invalid frame geometry"))?;
    let mut rgba = frame.to_rgba8();
    if opaque {
        make_opaque(&mut rgba);
    }

    render_rgb(&rgba, frame.width, frame.height, placement, output)
        .ok_or_else(|| unsupported("failed to render frame"))
}

/// Where a `source_width` x `source_height` picture (pixel aspect `par`) goes for `fit` on an `output` frame
/// shown at the shape `target`.
fn placement(
    source_width: u32,
    source_height: u32,
    par: (u32, u32),
    output: (u32, u32),
    target: Target,
    fit: SlideFit,
) -> Option<Placement> {
    let (output_width, output_height) = output;
    if source_width == 0 || source_height == 0 || output_height == 0 || output_width == 0 {
        return None;
    }
    match fit {
        SlideFit::ScaleAndMatte => {
            let geometry = target_geometry(source_width, source_height, par, output, target)?;
            Some(Placement {
                x: i64::from(geometry.x),
                y: i64::from(geometry.y),
                width: geometry.width,
                height: geometry.height,
            })
        }
        SlideFit::ScaleAndCrop => {
            let par_num = if par.0 == 0 { 1 } else { par.0 };
            let par_den = if par.1 == 0 { 1 } else { par.1 };
            let aspect_num = u128::from(source_width).checked_mul(u128::from(par_num))?;
            let aspect_den = u128::from(source_height).checked_mul(u128::from(par_den))?;
            let left = aspect_num.checked_mul(target.den)?;
            let right = target.num.checked_mul(aspect_den)?;
            if left == 0 || right == 0 {
                return None;
            }
            // Wider than the frame: as tall as the frame and wider; taller: as wide and taller.
            let (width, height) = if left >= right {
                let width = u128::from(output_width).checked_mul(left)? / right;
                (
                    clamp_u128(width, output_width.saturating_mul(64)),
                    output_height,
                )
            } else {
                let height = u128::from(output_height).checked_mul(right)? / left;
                (
                    output_width,
                    clamp_u128(height, output_height.saturating_mul(64)),
                )
            };
            Some(Placement {
                x: (i64::from(output_width) - i64::from(width)) / 2,
                y: (i64::from(output_height) - i64::from(height)) / 2,
                width: width.max(1),
                height: height.max(1),
            })
        }
        SlideFit::NoScale => Some(Placement {
            x: (i64::from(output_width) - i64::from(source_width)) / 2,
            y: (i64::from(output_height) - i64::from(source_height)) / 2,
            width: source_width,
            height: source_height,
        }),
    }
}

fn target_geometry(
    source_width: u32,
    source_height: u32,
    par: (u32, u32),
    output: (u32, u32),
    target: Target,
) -> Option<Geometry> {
    let (output_width, output_height) = output;
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
        (output_width, output_height)
    } else if left > right {
        let height = scaled_height(output_height, aspect_num, aspect_den, target)?;
        (output_width, height)
    } else {
        let width = scaled_width(output_width, aspect_num, aspect_den, target)?;
        (width, output_height)
    };

    let width = width.clamp(1, output_width.max(1));
    let height = height.clamp(1, output_height.max(1));
    let x = output_width.saturating_sub(width) / 2;
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

/// The width of a picture as tall as the frame: its share of the frame's width is its shape over the frame's
/// shown shape (`target`), whatever the frame's pixels are.
fn scaled_width(
    output_width: u32,
    aspect_num: u128,
    aspect_den: u128,
    target: Target,
) -> Option<u32> {
    let numerator = u128::from(output_width)
        .checked_mul(aspect_num)?
        .checked_mul(target.den)?;
    let denominator = aspect_den.checked_mul(target.num)?;
    if denominator == 0 {
        return Some(output_width);
    }

    Some(clamp_u128(numerator / denominator, output_width))
}

fn clamp_u128(value: u128, max: u32) -> u32 {
    if value > u128::from(max) {
        max
    } else {
        u32::try_from(value).unwrap_or(max)
    }
}

/// Draws the picture into a black frame of `output` at `placement`, cut to the frame.
fn render_rgb(
    rgba: &[u8],
    source_width: u32,
    source_height: u32,
    placement: Placement,
    output: (u32, u32),
) -> Option<Vec<u8>> {
    let (output_width, output_height) = output;
    let width_px = output_width as usize;
    let height = output_height as usize;
    let size = width_px.checked_mul(height)?.checked_mul(3)?;
    let mut rgb = vec![0u8; size];

    let start_x = placement.x.max(0);
    let start_y = placement.y.max(0);
    let end_x = placement
        .x
        .checked_add(i64::from(placement.width))?
        .min(i64::from(output_width));
    let end_y = placement
        .y
        .checked_add(i64::from(placement.height))?
        .min(i64::from(output_height));

    for y in start_y..end_y {
        for x in start_x..end_x {
            // Inside the frame and the placement, so these fit.
            let offset_x = u32::try_from(x.saturating_sub(placement.x)).ok()?;
            let offset_y = u32::try_from(y.saturating_sub(placement.y)).ok()?;
            let sx = source_coordinate(offset_x, 0, placement.width, source_width);
            let sy = source_coordinate(offset_y, 0, placement.height, source_height);
            let pixel = sample_bilinear(rgba, source_width, source_height, sx, sy)?;
            write_rgb(
                &mut rgb,
                width_px,
                u32::try_from(y).ok()?,
                u32::try_from(x).ok()?,
                pixel,
            )?;
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

fn write_silent_wav(path: &Path, sample_frames: usize) -> Result<(), TranscodeError> {
    let channels: u16 = 2;
    let bits: u16 = 16;

    let block_align = channels
        .checked_mul(bits / 8)
        .ok_or_else(|| unsupported("WAV block align overflow"))?;
    let byte_rate = audio::SAMPLE_RATE
        .checked_mul(u32::from(block_align))
        .ok_or_else(|| unsupported("WAV byte rate overflow"))?;

    let data_size = sample_frames
        .checked_mul(usize::from(block_align))
        .ok_or_else(|| unsupported("WAV data size overflow"))?;
    let data_size_u32 =
        u32::try_from(data_size).map_err(|_| unsupported("WAV data size too large"))?;
    let file_size = data_size_u32
        .checked_add(36)
        .ok_or_else(|| unsupported("WAV file size too large"))?;

    let file = File::create(path).map_err(|error| io_error(path, error))?;
    let mut writer = BufWriter::new(file);

    writer
        .write_all(b"RIFF")
        .map_err(|error| io_error(path, error))?;
    write_u32_le(path, &mut writer, file_size)?;
    writer
        .write_all(b"WAVE")
        .map_err(|error| io_error(path, error))?;
    writer
        .write_all(b"fmt ")
        .map_err(|error| io_error(path, error))?;
    write_u32_le(path, &mut writer, 16)?;
    write_u16_le(path, &mut writer, 1)?;
    write_u16_le(path, &mut writer, channels)?;
    write_u32_le(path, &mut writer, audio::SAMPLE_RATE)?;
    write_u32_le(path, &mut writer, byte_rate)?;
    write_u16_le(path, &mut writer, block_align)?;
    write_u16_le(path, &mut writer, bits)?;
    writer
        .write_all(b"data")
        .map_err(|error| io_error(path, error))?;
    write_u32_le(path, &mut writer, data_size_u32)?;

    let zeros = [0u8; 8192];
    let mut remaining = data_size;

    while remaining > 0 {
        let n = remaining.min(zeros.len());
        let chunk = zeros
            .get(..n)
            .ok_or_else(|| unsupported("zero chunk out of range"))?;
        writer
            .write_all(chunk)
            .map_err(|error| io_error(path, error))?;
        remaining -= n;
    }

    writer.flush().map_err(|error| io_error(path, error))?;
    Ok(())
}

fn write_u32_le<W: Write>(path: &Path, writer: &mut W, value: u32) -> Result<(), TranscodeError> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|error| io_error(path, error))
}

fn write_u16_le<W: Write>(path: &Path, writer: &mut W, value: u16) -> Result<(), TranscodeError> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|error| io_error(path, error))
}

#[cfg(test)]
mod tests {
    use super::{FOUR_THREE, Placement, SIXTEEN_NINE, SlideFit, placement, slide_rgb};

    /// M6.294: a slide whose alpha channel is ignored keeps its colours, every pixel opaque.
    #[test]
    fn an_ignored_alpha_channel_is_opaque() {
        let mut rgba = vec![200, 100, 50, 128, 10, 20, 30, 0];
        super::make_opaque(&mut rgba);
        assert_eq!(rgba, vec![200, 100, 50, 255, 10, 20, 30, 255]);
    }
    use super::{
        SlideTransition, Transition, cut_halves, transition_alpha, transition_frame,
        transition_half_frames,
    };
    use std::error::Error;

    /// A wide (2:1) vector slide is rendered sharp and letterboxed into the 4:3 frame.
    /// A zoom in starts with the picture as it is and ends enlarged about its centre; a pan without zoom keeps the
    /// enlarged size and travels from one side to the other; no motion leaves the picture alone.
    #[test]
    fn pan_and_zoom_move_the_picture() {
        use super::Motion;
        // A 16 x 8 frame, black with one white column at x = 12.
        let (width, height) = (16u32, 8u32);
        let mut rgb = vec![0u8; 16 * 8 * 3];
        for y in 0..8 {
            for channel in 0..3 {
                if let Some(value) = rgb.get_mut((y * 16 + 12) * 3 + channel) {
                    *value = 255;
                }
            }
        }
        // The x of the brightest pixel in the middle row.
        let bright = |frame: &[u8]| -> usize {
            (0..16)
                .max_by_key(|x| frame.get((4 * 16 + x) * 3).copied().unwrap_or(0))
                .unwrap_or(0)
        };
        let zoom_in = Motion {
            pan: (0, 0),
            zoom: 1,
        };
        assert_eq!(zoom_in.frame(&rgb, (width, height), 0.0), rgb);
        // Enlarged about the centre (x = 8): the column moves outwards.
        assert!(bright(&zoom_in.frame(&rgb, (width, height), 1.0)) >= 12);
        assert_eq!(zoom_in.at(0.0).0, 1.0);
        assert!((zoom_in.at(1.0).0 - super::MOTION_SCALE).abs() < 1e-6);
        let zoom_out = Motion {
            pan: (0, 0),
            zoom: -1,
        };
        assert_eq!(zoom_out.frame(&rgb, (width, height), 1.0), rgb);

        // The picture moving left: the view's centre goes from left of the middle to right of it.
        let left = Motion {
            pan: (-1, 0),
            zoom: 0,
        };
        let (scale, first, _) = left.at(0.0);
        let (_, last, _) = left.at(1.0);
        assert!((scale - super::MOTION_SCALE).abs() < 1e-6);
        assert!(first < 0.0 && last > 0.0 && (first + last).abs() < 1e-6);
        let right = Motion {
            pan: (1, 0),
            zoom: 0,
        };
        assert!(right.at(0.0).1 > 0.0 && right.at(1.0).1 < 0.0);
        let up = Motion {
            pan: (0, -1),
            zoom: 0,
        };
        assert!(up.at(0.0).2 < 0.0 && up.at(1.0).2 > 0.0 && up.at(0.5).1 == 0.0);

        let still = Motion {
            pan: (0, 0),
            zoom: 0,
        };
        assert_eq!(still.frame(&rgb, (width, height), 0.7), rgb);
    }

    #[test]
    fn slides_are_matted_cropped_or_left_alone() {
        // A 2:1 picture in the 4:3 frame (720 anamorphic pixels wide, so 2:1 is 320 of the 480 lines).
        assert_eq!(
            placement(
                1440,
                720,
                (1, 1),
                (720, 480),
                FOUR_THREE,
                SlideFit::ScaleAndMatte
            ),
            Some(Placement {
                x: 0,
                y: 80,
                width: 720,
                height: 320
            })
        );
        // Cropped: as tall as the frame, wider than it, centred (sticks out on both sides).
        assert_eq!(
            placement(
                1440,
                720,
                (1, 1),
                (720, 480),
                FOUR_THREE,
                SlideFit::ScaleAndCrop
            ),
            Some(Placement {
                x: -180,
                y: 0,
                width: 1080,
                height: 480
            })
        );
        // Not scaled: pixel for pixel, centred.
        assert_eq!(
            placement(360, 240, (1, 1), (720, 480), FOUR_THREE, SlideFit::NoScale),
            Some(Placement {
                x: 180,
                y: 120,
                width: 360,
                height: 240
            })
        );
        assert_eq!(
            placement(0, 240, (1, 1), (720, 480), FOUR_THREE, SlideFit::NoScale),
            None
        );
    }

    /// A square picture fills three quarters of a 4:3 frame's width (540 of the 720 anamorphic pixels), and
    /// 1080 of an HD frame's 1920 square ones. (Up to M10.6 the width came from the frame's height: 360.)
    #[test]
    fn a_tall_slide_is_pillarboxed_at_its_shape() {
        assert_eq!(
            placement(
                600,
                600,
                (1, 1),
                (720, 480),
                FOUR_THREE,
                SlideFit::ScaleAndMatte
            ),
            Some(Placement {
                x: 90,
                y: 0,
                width: 540,
                height: 480
            })
        );
        assert_eq!(
            placement(
                600,
                600,
                (1, 1),
                (1920, 1080),
                SIXTEEN_NINE,
                SlideFit::ScaleAndMatte
            ),
            Some(Placement {
                x: 420,
                y: 0,
                width: 1080,
                height: 1080
            })
        );
        // A 4:3 SD frame's own anamorphic picture (8:9 pixels) fills it.
        assert_eq!(
            placement(
                720,
                480,
                (8, 9),
                (720, 480),
                FOUR_THREE,
                SlideFit::ScaleAndMatte
            ),
            Some(Placement {
                x: 0,
                y: 0,
                width: 720,
                height: 480
            })
        );
    }

    #[test]
    fn transitions_blend_towards_the_next_slide() {
        let from = vec![0_u8, 0, 0, 0, 0, 0];
        let to = vec![200_u8, 200, 200, 200, 200, 200];
        assert_eq!(
            transition_frame(SlideTransition::CrossDissolve, &from, &to, 0.5, 2),
            vec![100; 6]
        );
        // The wipe shows the next slide on the left of the edge.
        assert_eq!(
            transition_frame(SlideTransition::Wipe, &from, &to, 0.5, 2),
            vec![200, 200, 200, 0, 0, 0]
        );
        // A dip to black is black in the middle.
        assert_eq!(
            transition_frame(SlideTransition::DipToBlack, &from, &to, 0.5, 2),
            vec![0; 6]
        );
        assert_eq!(
            transition_frame(SlideTransition::DipToBlack, &from, &to, 0.75, 2),
            vec![100; 6]
        );
    }

    #[test]
    fn a_transition_never_swallows_a_slide() {
        // 30 frames each; a 20-frame half is cut to 15 (half of the shorter neighbour), the first slide has none.
        assert_eq!(cut_halves(&[30, 30, 10], &[20, 20, 20]), vec![0, 15, 5]);
        // Each cut has its own wanted half (a slide with its own transition).
        assert_eq!(cut_halves(&[30, 30, 30], &[20, 4, 0]), vec![0, 4, 0]);
        assert_eq!(cut_halves(&[30], &[20]), vec![0]);
        assert_eq!(transition_half_frames(Transition::NONE, (30_000, 1001)), 0);
        assert_eq!(
            transition_half_frames(
                Transition {
                    kind: SlideTransition::CrossDissolve,
                    seconds: 2.0
                },
                (25, 1)
            ),
            25
        );
        // 23.976 for an HD Blu-ray disc: a one-second half is 24 frames.
        assert_eq!(
            transition_half_frames(
                Transition {
                    kind: SlideTransition::CrossDissolve,
                    seconds: 2.0
                },
                (24_000, 1001)
            ),
            24
        );
        assert!(transition_alpha(0, 15) > 0.0);
        assert!(transition_alpha(29, 15) < 1.0);
    }

    #[test]
    fn a_vector_slide_is_letterboxed() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-slide-svg-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let svg = dir.join("wide.svg");
        std::fs::write(
            &svg,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="#0f0"/></svg>"##,
        )?;
        let rgb = slide_rgb(
            &svg,
            (720, 480),
            FOUR_THREE,
            (SlideFit::ScaleAndMatte, false),
        )?;
        assert_eq!(rgb.len(), 720 * 480 * 3);
        let at = |x: usize, y: usize| {
            rgb.get((y * 720 + x) * 3..(y * 720 + x) * 3 + 3)
                .map(<[u8]>::to_vec)
        };
        // Black bars above and below, the drawing in the middle.
        assert_eq!(at(360, 10), Some(vec![0, 0, 0]));
        assert_eq!(at(360, 240), Some(vec![0, 255, 0]));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
