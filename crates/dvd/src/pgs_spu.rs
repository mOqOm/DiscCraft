//! M6.541: a Blu-ray's PGS subtitles (`.sup`, as one taken out of an MKV) on a DVD.
//!
//! A DVD's subpicture has four pixel values, each one of the title's 16 colours with a contrast of 16 steps, where a
//! PGS object has up to 256 colours each with its own alpha. So each picture a PGS track shows is drawn (its objects
//! at their places, through its palette), scaled to the DVD's frame, and its pixels are put into four values: the
//! transparent background and three colours found from all of the track's pictures together (so that the title's
//! 16 colours hold them), each value with the mean contrast of its pixels. White letters with a black edge, as most
//! Blu-ray subtitles are, come out as white, black and the grey between.
//!
//! The PGS layout is the one [`crate::sup`] passes through to a Blu-ray and `disccraft-pgs` writes: display sets of a
//! presentation composition (PCS: the video's size, the composition state, the palette and the objects shown with
//! their places and cropping), palette definitions (PDS: entries of Y, Cr, Cb and alpha), object definitions (ODS:
//! run-length coded, in fragments) and an END. An epoch start drops the definitions before it. A picture shows from
//! its PCS's time until the next PCS's (a PCS without objects clears the screen). Palettes of a video taller than 576
//! lines are BT.709, else BT.601.

use std::collections::BTreeMap;
use std::path::Path;

use crate::DvdError;

const PDS: u8 = 0x14;
const ODS: u8 = 0x15;
const PCS: u8 = 0x16;
const END: u8 = 0x80;
/// Composition state bit of an epoch start.
const EPOCH_START: u8 = 0x80;
/// Composition object flags.
const CROPPED: u8 = 0x80;
const FORCED: u8 = 0x40;
/// ODS sequence flags.
const FIRST_IN_SEQUENCE: u8 = 0x80;
const LAST_IN_SEQUENCE: u8 = 0x40;
/// The most objects an epoch holds, and the most bytes of one object's run-length data.
const MAX_OBJECTS: usize = 64;
const MAX_OBJECT_DATA: usize = 4 << 20;
/// The most pictures read from one track.
const MAX_PICTURES: usize = 100_000;
/// The most pixels of one object (a 4096 × 4096 plane).
const MAX_OBJECT_PIXELS: usize = 4096 * 4096;
/// Pixels less opaque than this are the background.
const SOLID: u8 = 64;
/// The most pixels the track's colours are found from.
const MAX_SAMPLES: usize = 200_000;
/// How long a picture no later PCS ends shows, 90 kHz (five seconds).
const LAST_SHOWS: i64 = 5 * 90_000;

/// A PGS track as a DVD's subpicture units: the palette its units' colour indices point into (RGB; entry 0 the
/// background) and each unit with its time (90 kHz, as the `.sup` has it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PgsUnits {
    pub(crate) palette: [[u8; 3]; 16],
    pub(crate) units: Vec<(i64, Vec<u8>)>,
}

/// One picture a track shows, scaled to the DVD's frame: from `pts` for `duration` (90 kHz), at `x`, `y`, its pixels
/// (straight RGBA).
struct Picture {
    pts: i64,
    duration: i64,
    forced: bool,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    rgba: Vec<[u8; 4]>,
}

/// A composition object of a PCS.
struct Shown {
    object: u16,
    x: u16,
    y: u16,
    crop: Option<[u16; 4]>,
    forced: bool,
}

/// A PCS waiting for its display set's END.
struct Composition {
    pts: i64,
    video: (u16, u16),
    palette: u8,
    objects: Vec<Shown>,
}

/// An object being read: its size and run-length data so far.
struct Partial {
    width: u16,
    height: u16,
    data: Vec<u8>,
}

/// Reads the PGS track `path` as subpicture units for a DVD whose frame is `frame` (720 × 480 or 576).
///
/// # Errors
///
/// When the file cannot be read or is not PGS.
pub(crate) fn read_pgs_for_dvd(path: &Path, frame: (u16, u16)) -> Result<PgsUnits, DvdError> {
    let segments = crate::sup::read_sup(path)?;
    let pictures = pictures(&segments, frame);
    units(&pictures).map_err(|error| DvdError::Invalid(format!("{}: {error}", path.display())))
}

/// The pictures the segments show, in time order, scaled to `frame`.
fn pictures(segments: &[crate::sup::PgSegment], frame: (u16, u16)) -> Vec<Picture> {
    let mut palettes: BTreeMap<u8, BTreeMap<u8, [u8; 4]>> = BTreeMap::new();
    let mut objects: BTreeMap<u16, (u16, u16, Vec<u8>)> = BTreeMap::new();
    let mut partial: BTreeMap<u16, Partial> = BTreeMap::new();
    let mut pending: Option<Composition> = None;
    // Each composition: its time and what it shows (nothing for one that clears the screen).
    let mut shown: Vec<(i64, Option<Picture>)> = Vec::new();
    for segment in segments {
        let Some((&kind, rest)) = segment.data.split_first() else {
            continue;
        };
        let body = rest.get(2..).unwrap_or_default();
        match kind {
            PCS => {
                let Some(composition) = parse_pcs(segment.pts, body) else {
                    continue;
                };
                if body.get(7).is_some_and(|state| state & EPOCH_START != 0) {
                    palettes.clear();
                    objects.clear();
                    partial.clear();
                }
                pending = Some(composition);
            }
            PDS => parse_pds(body, &mut palettes),
            ODS => parse_ods(body, &mut objects, &mut partial),
            END => {
                if let Some(composition) = pending.take() {
                    let colours = palettes
                        .get(&composition.palette)
                        .cloned()
                        .unwrap_or_default();
                    let hd = composition.video.1 > 576;
                    let picture = draw(&composition, &objects, &colours, hd, frame);
                    shown.push((composition.pts, picture));
                    if shown.len() >= MAX_PICTURES {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    shown.sort_by_key(|(pts, _)| *pts);
    let ends: Vec<Option<i64>> = (0..shown.len())
        .map(|at| shown.get(at + 1).map(|(pts, _)| *pts))
        .collect();
    shown
        .into_iter()
        .zip(ends)
        .filter_map(|((pts, picture), end)| {
            let mut picture = picture?;
            picture.duration = end.map_or(LAST_SHOWS, |end| end.saturating_sub(pts));
            (picture.duration > 0).then_some(picture)
        })
        .collect()
}

fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
}

/// A PCS body: width, height, frame rate, composition number (2), state, palette update flag, palette id, object
/// count, then each object: id (2), window, flags, x (2), y (2) and, when cropped, the crop's x, y, width, height.
fn parse_pcs(pts: i64, body: &[u8]) -> Option<Composition> {
    let width = be16(body, 0)?;
    let height = be16(body, 2)?;
    let palette = *body.get(9)?;
    let count = usize::from(*body.get(10)?);
    let mut objects = Vec::with_capacity(count.min(MAX_OBJECTS));
    let mut at = 11usize;
    for _ in 0..count.min(MAX_OBJECTS) {
        let object = be16(body, at)?;
        let flags = *body.get(at + 3)?;
        let x = be16(body, at + 4)?;
        let y = be16(body, at + 6)?;
        at += 8;
        let crop = if flags & CROPPED != 0 {
            let crop = [
                be16(body, at)?,
                be16(body, at + 2)?,
                be16(body, at + 4)?,
                be16(body, at + 6)?,
            ];
            at += 8;
            Some(crop)
        } else {
            None
        };
        objects.push(Shown {
            object,
            x,
            y,
            crop,
            forced: flags & FORCED != 0,
        });
    }
    (width > 0 && height > 0).then_some(Composition {
        pts,
        video: (width, height),
        palette,
        objects,
    })
}

/// A PDS body: palette id, version, then entries of id, Y, Cr, Cb, alpha (an entry updates the palette's own).
fn parse_pds(body: &[u8], palettes: &mut BTreeMap<u8, BTreeMap<u8, [u8; 4]>>) {
    let Some(&id) = body.first() else {
        return;
    };
    let palette = palettes.entry(id).or_default();
    for &[index, y, cr, cb, alpha] in body.get(2..).unwrap_or_default().as_chunks::<5>().0 {
        let _ = palette.insert(index, [y, cr, cb, alpha]);
    }
}

/// An ODS body: object id (2), version, sequence flags and, in the first fragment, the data's length (3), width (2)
/// and height (2) before the run-length data; later fragments carry more data. An object whose data is complete is
/// decoded to palette indices (left out when it does not decode).
fn parse_ods(
    body: &[u8],
    objects: &mut BTreeMap<u16, (u16, u16, Vec<u8>)>,
    partial: &mut BTreeMap<u16, Partial>,
) {
    let (Some(id), Some(&flags)) = (be16(body, 0), body.get(3)) else {
        return;
    };
    if flags & FIRST_IN_SEQUENCE != 0 {
        let (Some(width), Some(height)) = (be16(body, 7), be16(body, 9)) else {
            return;
        };
        let _ = partial.insert(
            id,
            Partial {
                width,
                height,
                data: body.get(11..).unwrap_or_default().to_vec(),
            },
        );
    } else if let Some(object) = partial.get_mut(&id) {
        object
            .data
            .extend_from_slice(body.get(4..).unwrap_or_default());
    }
    if partial
        .get(&id)
        .is_some_and(|object| object.data.len() > MAX_OBJECT_DATA)
    {
        let _ = partial.remove(&id);
        return;
    }
    if flags & LAST_IN_SEQUENCE != 0
        && let Some(object) = partial.remove(&id)
    {
        let pixels = usize::from(object.width).saturating_mul(usize::from(object.height));
        if pixels == 0 || pixels > MAX_OBJECT_PIXELS {
            return;
        }
        if objects.len() >= MAX_OBJECTS && !objects.contains_key(&id) {
            return;
        }
        if let Ok(indices) = disccraft_pgs::rle_decode(object.width, object.height, &object.data) {
            let _ = objects.insert(id, (object.width, object.height, indices));
        }
    }
}

/// A palette entry (Y, Cr, Cb, alpha) as straight RGBA (`hd`: BT.709, else BT.601; video range).
fn rgba([y, cr, cb, alpha]: [u8; 4], hd: bool) -> [u8; 4] {
    let y = 1.164 * (f64::from(y) - 16.0);
    let cr = f64::from(cr) - 128.0;
    let cb = f64::from(cb) - 128.0;
    let (r, g, b) = if hd {
        (y + 1.793 * cr, y - 0.213 * cb - 0.533 * cr, y + 2.112 * cb)
    } else {
        (y + 1.596 * cr, y - 0.392 * cb - 0.813 * cr, y + 2.017 * cb)
    };
    // `as` after clamping to a byte's range.
    let byte = |value: f64| value.round().clamp(0.0, 255.0) as u8;
    [byte(r), byte(g), byte(b), alpha]
}

/// The composition's objects drawn through the palette `colours` and scaled to `frame`; `None` when it shows nothing.
fn draw(
    composition: &Composition,
    objects: &BTreeMap<u16, (u16, u16, Vec<u8>)>,
    colours: &BTreeMap<u8, [u8; 4]>,
    hd: bool,
    frame: (u16, u16),
) -> Option<Picture> {
    // Each object's part shown: where it goes in the video, its size, and where in the object it starts.
    let mut parts = Vec::new();
    for shown in &composition.objects {
        let Some((width, height, indices)) = objects.get(&shown.object) else {
            continue;
        };
        let [cx, cy, cw, ch] = shown.crop.unwrap_or([0, 0, *width, *height]);
        let cw = cw.min(width.saturating_sub(cx));
        let ch = ch.min(height.saturating_sub(cy));
        if cw == 0 || ch == 0 {
            continue;
        }
        parts.push((shown.x, shown.y, cw, ch, cx, cy, *width, indices));
    }
    let left = parts.iter().map(|part| u32::from(part.0)).min()?;
    let top = parts.iter().map(|part| u32::from(part.1)).min()?;
    let right = parts
        .iter()
        .map(|part| u32::from(part.0) + u32::from(part.2))
        .max()?;
    let bottom = parts
        .iter()
        .map(|part| u32::from(part.1) + u32::from(part.3))
        .max()?;
    let (video_width, video_height) = composition.video;
    let right = right.min(u32::from(video_width));
    let bottom = bottom.min(u32::from(video_height));
    if right <= left || bottom <= top {
        return None;
    }
    let (source_width, source_height) = (right - left, bottom - top);
    let width = usize::try_from(source_width).ok()?;
    let mut canvas = vec![[0u8; 4]; width.checked_mul(usize::try_from(source_height).ok()?)?];
    let lookup: BTreeMap<u8, [u8; 4]> = colours
        .iter()
        .map(|(index, entry)| (*index, rgba(*entry, hd)))
        .collect();
    for (x, y, cw, ch, cx, cy, object_width, indices) in parts {
        for row in 0..u32::from(ch) {
            let into_y = u32::from(y) + row;
            if into_y < top || into_y >= bottom {
                continue;
            }
            for column in 0..u32::from(cw) {
                let into_x = u32::from(x) + column;
                if into_x < left || into_x >= right {
                    continue;
                }
                let from = usize::try_from(
                    (u32::from(cy) + row) * u32::from(object_width) + u32::from(cx) + column,
                )
                .ok()?;
                let Some(colour) = indices.get(from).and_then(|index| lookup.get(index)) else {
                    continue;
                };
                let to = usize::try_from((into_y - top) * source_width + (into_x - left)).ok()?;
                if let Some(pixel) = canvas.get_mut(to)
                    && colour[3] > 0
                {
                    *pixel = *colour;
                }
            }
        }
    }
    scale(
        &canvas,
        (left, top, source_width, source_height),
        composition,
        frame,
    )
}

/// `canvas` (the part `left`, `top`, `width` × `height` of the composition's video) scaled to `frame`, each pixel the
/// mean of the video's pixels it covers (colours weighted by their alpha).
fn scale(
    canvas: &[[u8; 4]],
    (left, top, width, height): (u32, u32, u32, u32),
    composition: &Composition,
    (frame_width, frame_height): (u16, u16),
) -> Option<Picture> {
    let (video_width, video_height) = composition.video;
    let sx = f64::from(frame_width) / f64::from(video_width);
    let sy = f64::from(frame_height) / f64::from(video_height);
    // `as` on values inside the frame.
    let x0 = (f64::from(left) * sx).floor() as u32;
    let y0 = (f64::from(top) * sy).floor() as u32;
    let x1 = ((f64::from(left + width) * sx).ceil() as u32).min(u32::from(frame_width));
    let y1 = ((f64::from(top + height) * sy).ceil() as u32).min(u32::from(frame_height));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let mut rgba = Vec::with_capacity(usize::try_from((x1 - x0) * (y1 - y0)).ok()?);
    // The video's pixels from `a` to `b` of the frame's (rounded to whole pixels, at least one), within the canvas.
    let span = |a: u32, b: u32, scale: f64, origin: u32, size: u32| {
        let from = ((f64::from(a) / scale).round() as u32).max(origin);
        let to = ((f64::from(b) / scale).round() as u32).max(from + 1);
        (from - origin, (to - origin).min(size))
    };
    for ty in y0..y1 {
        let (row_from, row_to) = span(ty, ty + 1, sy, top, height);
        for tx in x0..x1 {
            let (column_from, column_to) = span(tx, tx + 1, sx, left, width);
            let mut sum = [0.0f64; 4];
            let mut count = 0.0f64;
            for row in row_from..row_to {
                for column in column_from..column_to {
                    let Some(pixel) = usize::try_from(row * width + column)
                        .ok()
                        .and_then(|at| canvas.get(at))
                    else {
                        continue;
                    };
                    let alpha = f64::from(pixel[3]);
                    for (total, value) in sum.iter_mut().zip(&pixel[..3]) {
                        *total += f64::from(*value) * alpha;
                    }
                    sum[3] += alpha;
                    count += 1.0;
                }
            }
            let pixel = if sum[3] > 0.0 && count > 0.0 {
                // `as` after rounding values of a byte's range.
                [
                    (sum[0] / sum[3]).round() as u8,
                    (sum[1] / sum[3]).round() as u8,
                    (sum[2] / sum[3]).round() as u8,
                    (sum[3] / count).round() as u8,
                ]
            } else {
                [0; 4]
            };
            rgba.push(pixel);
        }
    }
    Some(Picture {
        pts: composition.pts,
        duration: 0,
        forced: !composition.objects.is_empty()
            && composition.objects.iter().all(|shown| shown.forced),
        x: u16::try_from(x0).ok()?,
        y: u16::try_from(y0).ok()?,
        width: u16::try_from(x1 - x0).ok()?,
        height: u16::try_from(y1 - y0).ok()?,
        rgba,
    })
}

/// The squared distance between two colours.
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(&b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// The track's three colours (RGB) and their pixels' mean alpha, found from all of its pictures' solid pixels: the
/// commonest colour, the one farthest from it, and the one farthest from both start a k-means that sorts the pixels
/// into three groups; each group's colour is then its commonest (so that white letters stay white and a black edge
/// black, not the mean of the blend at their border).
fn track_colours(pictures: &[Picture]) -> [([f64; 3], f64); 3] {
    let solid: Vec<[f64; 4]> = pictures
        .iter()
        .flat_map(|picture| picture.rgba.iter())
        .filter(|pixel| pixel[3] >= SOLID)
        .map(|pixel| pixel.map(f64::from))
        .collect();
    let step = solid.len().div_ceil(MAX_SAMPLES).max(1);
    let samples: Vec<[f64; 4]> = solid.into_iter().step_by(step).collect();
    let colour = |pixel: &[f64; 4]| [pixel[0], pixel[1], pixel[2]];
    let Some(first) = commonest(samples.iter()) else {
        return [([255.0; 3], 255.0), ([0.0; 3], 255.0), ([128.0; 3], 255.0)];
    };
    let farthest = |from: &[[f64; 3]]| {
        samples
            .iter()
            .map(|pixel| {
                let near = from
                    .iter()
                    .map(|centre| distance(*centre, colour(pixel)))
                    .fold(f64::INFINITY, f64::min);
                (near, colour(pixel))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map_or(first, |(_, colour)| colour)
    };
    let second = farthest(&[first]);
    let third = farthest(&[first, second]);
    let mut centres = [first, second, third];
    for _ in 0..12 {
        let mut sums = [[0.0f64; 4]; 3];
        for pixel in &samples {
            if let Some(sum) = sums.get_mut(nearest(&centres, colour(pixel))) {
                for (total, value) in sum.iter_mut().zip(colour(pixel)) {
                    *total += value * pixel[3];
                }
                sum[3] += pixel[3];
            }
        }
        for (centre, sum) in centres.iter_mut().zip(sums) {
            if sum[3] > 0.0 {
                *centre = [sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3]];
            }
        }
    }
    let mut out = [([0.0; 3], 255.0); 3];
    for (group, slot) in out.iter_mut().enumerate() {
        let members: Vec<&[f64; 4]> = samples
            .iter()
            .filter(|pixel| nearest(&centres, colour(pixel)) == group)
            .collect();
        let centre = centres.get(group).copied().unwrap_or([0.0; 3]);
        let alpha = if members.is_empty() {
            255.0
        } else {
            // `as`: a count of pixels.
            members.iter().map(|pixel| pixel[3]).sum::<f64>() / members.len() as f64
        };
        *slot = (commonest(members.into_iter()).unwrap_or(centre), alpha);
    }
    out
}

/// The commonest colour of `pixels` (weighted by alpha): the mean of the pixels in the fullest of 16 × 16 × 16 boxes.
fn commonest<'a>(pixels: impl Iterator<Item = &'a [f64; 4]> + Clone) -> Option<[f64; 3]> {
    // `as` on colour values of a byte's range.
    let bin = |pixel: &[f64; 4]| {
        let level = |value: f64| (value.clamp(0.0, 255.0) as usize) / 16;
        (level(pixel[0]) * 16 + level(pixel[1])) * 16 + level(pixel[2])
    };
    let mut weights = vec![0.0f64; 4096];
    for pixel in pixels.clone() {
        if let Some(weight) = weights.get_mut(bin(pixel)) {
            *weight += pixel[3];
        }
    }
    let (fullest, weight) = weights
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))?;
    if *weight <= 0.0 {
        return None;
    }
    let mut sum = [0.0f64; 4];
    for pixel in pixels.filter(|pixel| bin(pixel) == fullest) {
        for (total, value) in sum.iter_mut().zip(pixel.iter().take(3)) {
            *total += value * pixel[3];
        }
        sum[3] += pixel[3];
    }
    (sum[3] > 0.0).then(|| [sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3]])
}

/// The centre nearest `colour`.
fn nearest(centres: &[[f64; 3]; 3], colour: [f64; 3]) -> usize {
    let mut best = 0usize;
    let mut best_distance = f64::INFINITY;
    for (index, centre) in centres.iter().enumerate() {
        let d = distance(*centre, colour);
        if d < best_distance {
            best = index;
            best_distance = d;
        }
    }
    best
}

/// The pictures as subpicture units: pixel value 0 the background, 1 to 3 the track's colours.
fn units(pictures: &[Picture]) -> Result<PgsUnits, String> {
    let colours = track_colours(pictures);
    let mut palette = [[0u8; 3]; 16];
    let mut alpha = [0u8; 4];
    for (index, (centre, mean)) in colours.iter().enumerate() {
        // `as` after rounding values of a byte's range.
        if let Some(slot) = palette.get_mut(index + 1) {
            *slot = centre.map(|value| value.round().clamp(0.0, 255.0) as u8);
        }
        if let Some(slot) = alpha.get_mut(index + 1) {
            *slot = ((mean * 15.0 / 255.0).round().clamp(1.0, 15.0)) as u8;
        }
    }
    let centres = [colours[0].0, colours[1].0, colours[2].0];
    let mut units = Vec::with_capacity(pictures.len());
    for picture in pictures {
        let pixels: Vec<u8> = picture
            .rgba
            .iter()
            .map(|pixel| {
                if pixel[3] < SOLID {
                    0
                } else {
                    let colour = [pixel[0], pixel[1], pixel[2]].map(f64::from);
                    // 1 to 3: `nearest` is below 3.
                    u8::try_from(nearest(&centres, colour) + 1).unwrap_or(1)
                }
            })
            .collect();
        let bitmap = disccraft_spu::Bitmap {
            width: picture.width,
            height: picture.height,
            pixels,
        };
        let params = disccraft_spu::SpuParams {
            x: picture.x,
            y: picture.y,
            colors: [0, 1, 2, 3],
            alpha,
            forced: picture.forced,
            stop_delay: Some(disccraft_spu::spu_delay_from_90k(
                u32::try_from(picture.duration).unwrap_or(u32::MAX),
            )),
        };
        let spu = disccraft_spu::encode_spu(&bitmap, &params).map_err(|error| error.0)?;
        units.push((picture.pts, spu));
    }
    Ok(PgsUnits { palette, units })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{PgsUnits, pictures, units};
    use crate::sup::PgSegment;

    /// A 1920 × 1080 PGS track of `subtitles` (as `disccraft-pgs` writes a Blu-ray's), read back as segments.
    fn segments(subtitles: &[disccraft_pgs::Subtitle]) -> Vec<PgSegment> {
        disccraft_pgs::encode_subtitles(subtitles, 1920, 1080)
            .unwrap()
            .into_iter()
            .map(|segment| PgSegment {
                pts: i64::try_from(segment.pts).unwrap(),
                dts: i64::try_from(segment.dts).unwrap(),
                data: segment.data,
            })
            .collect()
    }

    /// White letters with a black edge: a 200 × 40 white box inside a black frame 4 pixels wide, on nothing.
    fn letters(start: u64, end: u64) -> disccraft_pgs::Subtitle {
        let (width, height) = (208u16, 48u16);
        let mut pixels = vec![0u8; usize::from(width) * usize::from(height)];
        for y in 0..usize::from(height) {
            for x in 0..usize::from(width) {
                let edge = x < 4 || y < 4 || x >= 204 || y >= 44;
                pixels[y * usize::from(width) + x] = if edge { 2 } else { 1 };
            }
        }
        disccraft_pgs::Subtitle {
            forced: false,
            start,
            end,
            x: 856,
            y: 900,
            width,
            height,
            pixels,
            palette: vec![
                disccraft_pgs::rgba_to_entry(0, 0, 0, 0, 0),
                disccraft_pgs::rgba_to_entry(1, 255, 255, 255, 255),
                disccraft_pgs::rgba_to_entry(2, 0, 0, 0, 255),
            ],
        }
    }

    /// A Blu-ray's subtitles on a DVD (M6.541): each shows at its time until the screen clears, scaled from 1920 ×
    /// 1080 to 720 × 480 at its place, in white and black.
    #[test]
    fn a_blurays_subtitles_become_a_dvds() {
        let segments = segments(&[letters(90_000, 270_000), letters(450_000, 540_000)]);
        let shown = pictures(&segments, (720, 480));
        assert_eq!(shown.len(), 2);
        assert_eq!((shown[0].pts, shown[0].duration), (90_000, 180_000));
        assert_eq!((shown[1].pts, shown[1].duration), (450_000, 90_000));
        // 856 × 0.375 = 321, 900 × 4/9 = 400; 208 × 0.375 = 78, 48 × 4/9 ≈ 22.
        assert_eq!((shown[0].x, shown[0].y), (321, 400));
        assert_eq!((shown[0].width, shown[0].height), (78, 22));
        let PgsUnits { palette, units } = units(&shown).unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].0, 90_000);
        let colours: Vec<[u8; 3]> = palette[1..4].to_vec();
        let near = |want: [u8; 3]| {
            colours
                .iter()
                .any(|got| got.iter().zip(&want).all(|(a, b)| a.abs_diff(*b) <= 12))
        };
        assert!(near([255, 255, 255]), "{colours:?}");
        assert!(near([0, 0, 0]), "{colours:?}");
        let decoded = disccraft_spu::decode_spu(&units[0].1).unwrap();
        assert_eq!((decoded.params.x, decoded.params.y), (321, 400));
        assert_eq!((decoded.bitmap.width, decoded.bitmap.height), (78, 22));
        // The middle is the white, the corner the black; both solid.
        let at = |x: usize, y: usize| decoded.bitmap.pixels[y * 78 + x];
        let white = at(39, 11);
        let black = at(0, 0);
        assert_ne!(white, black);
        assert!(white != 0 && black != 0);
        assert!(near(
            palette[usize::from(decoded.params.colors[usize::from(white)])]
        ));
        assert!(
            palette[usize::from(decoded.params.colors[usize::from(white)])]
                .iter()
                .map(|value| u32::from(*value))
                .sum::<u32>()
                > 600
        );
        assert_eq!(decoded.params.alpha[usize::from(white)], 15);
        assert!(decoded.params.stop_delay.is_some());
    }

    /// A track that shows nothing gives no units; segments cut off are read past.
    #[test]
    fn nothing_shown_is_no_unit() {
        let mut segments = segments(&[letters(90_000, 180_000)]);
        for segment in &mut segments {
            segment.data.truncate(3);
        }
        assert!(pictures(&segments, (720, 480)).is_empty());
        assert!(units(&[]).unwrap().units.is_empty());
    }
}
