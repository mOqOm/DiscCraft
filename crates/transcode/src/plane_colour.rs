//! M12.50: a decoded picture's Y'CbCr made the HD stream's (8-bit 4:2:0, BT.709 at 16–235) in its planes, without
//! going through RGB: the planes brought to 8 bits and 4:2:0 ([`plane_picture`]), then the colours moved from the
//! picture's matrix and range to BT.709's by an affine map of the samples ([`ColourMap`]). Turning the picture into
//! RGB and back took 3 to 9 of a 1080p frame's milliseconds (full range, 10 bits, 4:2:2), and the hardware encoder
//! waited on it.
//!
//! The map is the one the RGB path makes (the picture's Y'CbCr to R'G'B' by its matrix, R'G'B' to BT.709's Y'CbCr),
//! folded into one step: grey stays grey, so the luma is the picture's luma rescaled plus a part of its chroma (the
//! two matrices weigh the colours differently), and each chroma is a mix of the two chromas. The luma's part of the
//! chroma is interpolated to each pixel with each chroma sample between its luma samples, as the RGB path takes it.
//! Colours outside R'G'B' are kept rather than clipped.

use filmcraft_color::{ColorInfo, Matrix, Primaries, Range, Transfer};
use filmcraft_frame::{Chroma, PixelData, VideoFrame};
use rayon::prelude::*;
use std::borrow::Cow;

/// The fixed-point unit of the map's coefficients (16.16).
const ONE: f64 = 65_536.0;

/// Half of [`ONE`], for rounding.
const HALF: i32 = 1 << 15;

/// The fewest rows worth a thread of their own.
const ROWS_PER_WORKER: usize = 32;

/// A decoded picture's Y'CbCr as 8-bit 4:2:0 planes, still in its own colours.
pub(crate) struct PlanePicture<'a> {
    pub(crate) y: Cow<'a, [u8]>,
    pub(crate) u: Cow<'a, [u8]>,
    pub(crate) v: Cow<'a, [u8]>,
    pub(crate) width: usize,
    pub(crate) height: usize,
}

/// `frame`'s planes as 8-bit 4:2:0: 8-bit 4:2:0 as decoded; 4:2:2 with each two chroma lines averaged into one;
/// 9 to 16 bits rounded to 8. `None` (rendered through RGB): RGB, 4:4:4, an alpha plane, an odd size, or planes that
/// do not fit the size.
pub(crate) fn plane_picture(frame: &VideoFrame) -> Option<PlanePicture<'_>> {
    let width = usize::try_from(frame.width).ok()?;
    let height = usize::try_from(frame.height).ok()?;
    if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
        return None;
    }
    let luma = width.checked_mul(height)?;
    let quarter = (width / 2).checked_mul(height / 2)?;
    let chroma_size = |chroma: Chroma| match chroma {
        Chroma::C420 => Some(quarter),
        Chroma::C422 => quarter.checked_mul(2),
        Chroma::C444 => None,
    };
    let picture = |y, u, v| PlanePicture {
        y,
        u,
        v,
        width,
        height,
    };
    match &frame.data {
        PixelData::Yuv8 {
            planes: [y, u, v],
            chroma,
            alpha: None,
        } => {
            let size = chroma_size(*chroma)?;
            if y.len() != luma || u.len() != size || v.len() != size {
                return None;
            }
            Some(picture(
                Cow::Borrowed(y.as_slice()),
                chroma8(u, *chroma, width / 2),
                chroma8(v, *chroma, width / 2),
            ))
        }
        PixelData::Yuv16 {
            planes: [y, u, v],
            chroma,
            bits,
            alpha: None,
        } => {
            if !(8..=16).contains(bits) {
                return None;
            }
            let size = chroma_size(*chroma)?;
            if y.len() != luma || u.len() != size || v.len() != size {
                return None;
            }
            let shift = bits - 8;
            let chroma_plane = |plane: &[u16]| -> Vec<u8> {
                match chroma {
                    Chroma::C422 => pair_lines(plane, width / 2, |a, b| {
                        narrow(u32::from(a) + u32::from(b), shift + 1)
                    }),
                    _ => plane
                        .par_iter()
                        .map(|sample| narrow(u32::from(*sample), shift))
                        .collect(),
                }
            };
            let luma: Vec<u8> = y
                .par_iter()
                .map(|sample| narrow(u32::from(*sample), shift))
                .collect();
            Some(picture(
                Cow::Owned(luma),
                Cow::Owned(chroma_plane(u)),
                Cow::Owned(chroma_plane(v)),
            ))
        }
        _ => None,
    }
}

/// An 8-bit chroma plane as 4:2:0 (`width` samples a row): 4:2:0 as it is, 4:2:2 with each two lines averaged.
fn chroma8(plane: &[u8], chroma: Chroma, width: usize) -> Cow<'_, [u8]> {
    match chroma {
        Chroma::C422 => Cow::Owned(pair_lines(plane, width, |a, b| {
            narrow(u32::from(a) + u32::from(b), 1)
        })),
        _ => Cow::Borrowed(plane),
    }
}

/// `value` with its lowest `shift` bits rounded away, at most 255.
fn narrow(value: u32, shift: u32) -> u8 {
    let half = (1_u32 << shift) >> 1;
    u8::try_from((value.saturating_add(half) >> shift).min(255)).unwrap_or(u8::MAX)
}

/// A 4:2:2 chroma plane (`width` samples a row) as 4:2:0: each two lines made one by `pair`.
fn pair_lines<T: Copy + Send + Sync>(
    plane: &[T],
    width: usize,
    pair: impl Fn(T, T) -> u8 + Send + Sync,
) -> Vec<u8> {
    let mut out = vec![0_u8; plane.len() / 2];
    if width == 0 {
        return out;
    }
    out.par_chunks_mut(width)
        .zip(plane.par_chunks(width.saturating_mul(2)))
        .for_each(|(row, lines)| {
            let (top, bottom) = lines.split_at(width.min(lines.len()));
            for ((out, a), b) in row.iter_mut().zip(top).zip(bottom) {
                *out = pair(*a, *b);
            }
        });
    out
}

/// How a picture's 8-bit Y'CbCr becomes BT.709's at 16–235: 16.16 fixed-point coefficients of its samples less their
/// black (the luma) and middle (the chroma).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ColourMap {
    /// The picture's black: 16 at limited range, 0 at full (what its bars are filled with).
    pub(crate) black: u8,
    /// The luma's own coefficient.
    luma: i32,
    /// The luma's part of each chroma (zero when the matrix stays BT.709).
    luma_chroma: [i32; 2],
    /// Cb from the two chromas, and Cr.
    cb: [i32; 2],
    cr: [i32; 2],
}

/// The map from `color` to BT.709 at 16–235. `None`: colours the planes do not take (BT.2020, wide primaries, an HDR
/// or linear transfer), which are rendered through RGB. `Some(None)`: already BT.709 at 16–235.
pub(crate) fn colour_map(color: ColorInfo) -> Option<Option<ColourMap>> {
    if color.transfer != Transfer::Bt709
        || !matches!(
            color.primaries,
            Primaries::Bt709 | Primaries::Bt601_525 | Primaries::Bt601_625
        )
    {
        return None;
    }
    let (kr, kb) = match color.matrix {
        Matrix::Bt601 => (0.299, 0.114),
        Matrix::Bt709 => (0.2126, 0.0722),
        Matrix::Bt2020Ncl => return None,
    };
    if color.matrix == Matrix::Bt709 && color.range == Range::Limited {
        return Some(None);
    }
    let (black, luma_span, chroma_span) = match color.range {
        Range::Limited => (16_u8, 219.0, 224.0),
        Range::Full => (0_u8, 255.0, 255.0),
    };
    // The picture's normalised Y' (0 to 1) and Pb, Pr (-0.5 to 0.5) as BT.709's, through R'G'B'.
    let to_bt709 = |y: f64, pb: f64, pr: f64| -> [f64; 3] {
        let r = y + 2.0 * (1.0 - kr) * pr;
        let b = y + 2.0 * (1.0 - kb) * pb;
        let g = (y - kr * r - kb * b) / (1.0 - kr - kb);
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        [luma, (b - luma) / 1.8556, (r - luma) / 1.5748]
    };
    let from_pb = to_bt709(0.0, 1.0, 0.0);
    let from_pr = to_bt709(0.0, 0.0, 1.0);
    let fixed = |value: f64| (value * ONE).round() as i32;
    Some(Some(ColourMap {
        black,
        luma: fixed(219.0 / luma_span),
        luma_chroma: [
            fixed(219.0 * from_pb[0] / chroma_span),
            fixed(219.0 * from_pr[0] / chroma_span),
        ],
        cb: [
            fixed(224.0 * from_pb[1] / chroma_span),
            fixed(224.0 * from_pr[1] / chroma_span),
        ],
        cr: [
            fixed(224.0 * from_pb[2] / chroma_span),
            fixed(224.0 * from_pr[2] / chroma_span),
        ],
    }))
}

/// A fixed-point value rounded to a sample.
fn sample(value: i32) -> u8 {
    u8::try_from((value.saturating_add(HALF) >> 16).clamp(0, 255)).unwrap_or(u8::MAX)
}

impl ColourMap {
    /// Moves 4:2:0 planes of `width` x `height` (even) from the picture's colours to BT.709 at 16–235, in place.
    /// `None` when the planes do not fit the size.
    pub(crate) fn apply(
        &self,
        (y, u, v): (&mut [u8], &mut [u8], &mut [u8]),
        (width, height): (usize, usize),
    ) -> Option<()> {
        let (cw, ch) = (width / 2, height / 2);
        let quarter = cw.checked_mul(ch)?;
        let pixels = width.checked_mul(height)?;
        if y.len() < pixels || u.len() < quarter || v.len() < quarter {
            return None;
        }
        if cw == 0 || ch == 0 {
            return Some(());
        }
        let (u, v) = (u.get_mut(..quarter)?, v.get_mut(..quarter)?);
        // The luma's part of each chroma sample, then the chroma itself.
        let mixes = self.luma_chroma != [0, 0];
        let mut part = vec![0_i32; if mixes { quarter } else { 0 }];
        let rows = ROWS_PER_WORKER.max(1);
        let chroma_rows = u
            .par_chunks_mut(cw.saturating_mul(rows))
            .zip(v.par_chunks_mut(cw.saturating_mul(rows)));
        if mixes {
            chroma_rows
                .zip(part.par_chunks_mut(cw.saturating_mul(rows)))
                .for_each(|((u, v), part)| {
                    for ((cb, cr), part) in u.iter_mut().zip(v.iter_mut()).zip(part.iter_mut()) {
                        let (b, r) = (i32::from(*cb) - 128, i32::from(*cr) - 128);
                        *part = self.luma_chroma[0] * b + self.luma_chroma[1] * r;
                        *cb = sample(self.cb[0] * b + self.cb[1] * r + (128 << 16));
                        *cr = sample(self.cr[0] * b + self.cr[1] * r + (128 << 16));
                    }
                });
        } else {
            chroma_rows.for_each(|(u, v)| {
                for (cb, cr) in u.iter_mut().zip(v.iter_mut()) {
                    let (b, r) = (i32::from(*cb) - 128, i32::from(*cr) - 128);
                    *cb = sample(self.cb[0] * b + self.cb[1] * r + (128 << 16));
                    *cr = sample(self.cr[0] * b + self.cr[1] * r + (128 << 16));
                }
            });
        }
        let black = i32::from(self.black);
        let part = &part;
        y.get_mut(..pixels)?
            .par_chunks_mut(width)
            .enumerate()
            .for_each(|(row, luma)| {
                // A chroma sample lies between its two luma samples each way, so a pixel takes three quarters of its
                // own sample and one of the neighbour on its other side, across and down.
                let own = (row / 2).min(ch - 1);
                let other = if row % 2 == 0 {
                    own.saturating_sub(1)
                } else {
                    (own + 1).min(ch - 1)
                };
                let line = |index: usize| part.get(index * cw..(index + 1) * cw);
                let lines = if mixes {
                    line(own).zip(line(other))
                } else {
                    None
                };
                for (x, value) in luma.iter_mut().enumerate() {
                    let mut out = (i32::from(*value) - black) * self.luma + (16 << 16);
                    if let Some((near, far)) = lines {
                        let own = (x / 2).min(cw - 1);
                        let beside = if x % 2 == 0 {
                            own.saturating_sub(1)
                        } else {
                            (own + 1).min(cw - 1)
                        };
                        let at = |line: &[i32]| -> i32 {
                            let sample = |index: usize| line.get(index).copied().unwrap_or(0);
                            3 * sample(own) + sample(beside)
                        };
                        out += (3 * at(near) + at(far)) / 16;
                    }
                    *value = sample(out);
                }
            });
        Some(())
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn frame(data: PixelData, (width, height): (u32, u32), color: ColorInfo) -> VideoFrame {
        VideoFrame {
            width,
            height,
            data,
            color,
            par: (1, 1),
            pts: filmcraft_time::Tick(0),
        }
    }

    /// 4:2:2 chroma lines are averaged in pairs, 10-bit samples rounded to 8 (4:2:0 and 4:2:2), 8-bit 4:2:0 is
    /// borrowed as it is; 4:4:4, alpha and planes that do not fit are not taken.
    #[test]
    fn planes_are_brought_to_eight_bit_four_two_zero() {
        let color = ColorInfo::default();
        let size = (4, 2);
        let yuv8 = |chroma, u: Vec<u8>| PixelData::Yuv8 {
            planes: [Arc::new(vec![50; 8]), Arc::new(u.clone()), Arc::new(u)],
            chroma,
            alpha: None,
        };
        let same = frame(yuv8(Chroma::C420, vec![10, 20]), size, color);
        let planes = plane_picture(&same).unwrap();
        assert!(matches!(planes.u, Cow::Borrowed(_)));
        assert_eq!(&*planes.u, &[10, 20]);
        let four_two_two = frame(yuv8(Chroma::C422, vec![10, 20, 13, 30]), size, color);
        assert_eq!(&*plane_picture(&four_two_two).unwrap().u, &[12, 25]);
        let ten = |chroma, u: Vec<u16>| PixelData::Yuv16 {
            planes: [
                Arc::new(vec![64, 65, 66, 1023, 940, 941, 942, 0]),
                Arc::new(u.clone()),
                Arc::new(u),
            ],
            chroma,
            bits: 10,
            alpha: None,
        };
        let ten_420 = frame(ten(Chroma::C420, vec![512, 514]), size, color);
        let planes = plane_picture(&ten_420).unwrap();
        assert_eq!(&*planes.y, &[16, 16, 17, 255, 235, 235, 236, 0]);
        assert_eq!(&*planes.u, &[128, 129]);
        let ten_422 = frame(ten(Chroma::C422, vec![512, 514, 516, 1023]), size, color);
        let planes = plane_picture(&ten_422).unwrap();
        assert_eq!(&*planes.u, &[129, 192]);
        assert!(plane_picture(&frame(yuv8(Chroma::C444, vec![0; 8]), size, color)).is_none());
        assert!(plane_picture(&frame(yuv8(Chroma::C420, vec![0; 3]), size, color)).is_none());
        let mut with_alpha = same.clone();
        if let PixelData::Yuv8 { alpha, .. } = &mut with_alpha.data {
            *alpha = Some(Arc::new(vec![255; 8]));
        }
        assert!(plane_picture(&with_alpha).is_none());
        assert!(plane_picture(&frame(yuv8(Chroma::C420, vec![0; 2]), (3, 2), color)).is_none());
    }

    /// BT.709 at 16–235 needs no map; BT.2020, wide primaries and HDR transfers are not taken; BT.601 and full range
    /// keep black black, white white and grey grey, and map the colour bars' primaries where BT.709's matrix puts them.
    #[test]
    fn the_map_takes_each_colour_where_bt709_puts_it() {
        let hd = ColorInfo::default();
        assert_eq!(colour_map(hd), Some(None));
        for other in [
            ColorInfo {
                matrix: Matrix::Bt2020Ncl,
                ..hd
            },
            ColorInfo {
                primaries: Primaries::Bt2020,
                ..hd
            },
            ColorInfo {
                transfer: Transfer::Pq,
                ..hd
            },
        ] {
            assert_eq!(colour_map(other), None, "{other:?}");
        }
        let bt709 = |[r, g, b]: [f64; 3]| -> [f64; 3] {
            let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            [
                16.0 + 219.0 * y,
                128.0 + 224.0 * (b - y) / 1.8556,
                128.0 + 224.0 * (r - y) / 1.5748,
            ]
        };
        for (matrix, (kr, kb)) in [
            (Matrix::Bt601, (0.299, 0.114)),
            (Matrix::Bt709, (0.2126, 0.0722)),
        ] {
            for range in [Range::Limited, Range::Full] {
                let color = ColorInfo {
                    matrix,
                    range,
                    ..hd
                };
                let Some(Some(map)) = colour_map(color) else {
                    assert_eq!((matrix, range), (Matrix::Bt709, Range::Limited));
                    continue;
                };
                let (black, luma, chroma) = match range {
                    Range::Limited => (16.0, 219.0, 224.0),
                    Range::Full => (0.0, 255.0, 255.0),
                };
                for rgb in [
                    [0.0, 0.0, 0.0],
                    [1.0, 1.0, 1.0],
                    [0.5, 0.5, 0.5],
                    [0.75, 0.0, 0.0],
                    [0.0, 0.75, 0.0],
                    [0.0, 0.0, 0.75],
                    [0.75, 0.75, 0.0],
                ] {
                    // The colour in the picture's own Y'CbCr, a flat 2 x 2.
                    let [r, g, b] = rgb;
                    let y = kr * r + (1.0 - kr - kb) * g + kb * b;
                    let at = |value: f64| value.round().clamp(0.0, 255.0) as u8;
                    let mut planes = (
                        vec![at(black + luma * y); 4],
                        vec![at(128.0 + chroma * (b - y) / (2.0 * (1.0 - kb)))],
                        vec![at(128.0 + chroma * (r - y) / (2.0 * (1.0 - kr)))],
                    );
                    map.apply((&mut planes.0, &mut planes.1, &mut planes.2), (2, 2))
                        .unwrap();
                    let expected = bt709(rgb);
                    let got = [planes.0[0], planes.1[0], planes.2[0]];
                    for (got, expected) in got.iter().zip(expected) {
                        assert!(
                            (f64::from(*got) - expected).abs() <= 1.5,
                            "{matrix:?} {range:?} {rgb:?}: {got:?} vs {expected:?}"
                        );
                    }
                }
            }
        }
    }
}
