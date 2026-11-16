//! VobSub (`.idx` + `.sub`): a DVD's own subtitles as files (the subpicture units of a DVD, their palette and their
//! times), as ffmpeg, VobSubRip and other tools write them from a disc. A DVD build passes the units through as they
//! are; only their four colour indices are moved to the title's palette ([`PaletteBuilder`]).

use std::path::{Path, PathBuf};

use crate::DvdError;

/// The first stream of a VobSub: its palette (RGB), language, and subpicture units with their times (ms from the
/// video's first picture, the file's delay and time offset applied).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VobSub {
    pub(crate) palette: [[u8; 3]; 16],
    pub(crate) language: Option<[u8; 2]>,
    pub(crate) units: Vec<(i64, Vec<u8>)>,
}

fn invalid(path: &Path, what: &str) -> DvdError {
    DvdError::Invalid(format!("{}: {what}", path.display()))
}

/// Reads a VobSub's first stream from its `.idx` (and the `.sub` beside it).
///
/// # Errors
///
/// When either file cannot be read or is not VobSub.
pub(crate) fn read_vobsub(idx: &Path) -> Result<VobSub, DvdError> {
    let text = std::fs::read_to_string(idx).map_err(|err| crate::io_error(idx, err))?;
    let sub_path: PathBuf = idx.with_extension("sub");
    let sub = std::fs::read(&sub_path).map_err(|err| crate::io_error(&sub_path, err))?;

    let mut palette = None;
    let mut language = None;
    let mut stream: Option<u8> = None;
    let mut offset_ms = 0i64;
    let mut entries: Vec<(i64, usize)> = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("palette:") {
            palette = Some(parse_palette(rest).ok_or_else(|| invalid(idx, "bad palette"))?);
        } else if let Some(rest) = line.strip_prefix("time offset:") {
            offset_ms = parse_signed_ms(rest).ok_or_else(|| invalid(idx, "bad time offset"))?;
        } else if let Some(rest) = line.strip_prefix("id:") {
            // Only the first stream: `id: en, index: 0`.
            if stream.is_some() {
                break;
            }
            let (lang, index) = rest
                .split_once(", index:")
                .ok_or_else(|| invalid(idx, "bad id line"))?;
            language = parse_language(lang.trim());
            stream = Some(
                index
                    .trim()
                    .parse()
                    .map_err(|_| invalid(idx, "bad stream index"))?,
            );
        } else if let Some(rest) = line.strip_prefix("delay:") {
            offset_ms += parse_signed_ms(rest).ok_or_else(|| invalid(idx, "bad delay"))?;
        } else if let Some(rest) = line.strip_prefix("timestamp:") {
            let (time, pos) = rest
                .split_once(", filepos:")
                .ok_or_else(|| invalid(idx, "bad timestamp line"))?;
            let ms = parse_signed_ms(time).ok_or_else(|| invalid(idx, "bad timestamp"))?;
            let pos =
                usize::from_str_radix(pos.trim(), 16).map_err(|_| invalid(idx, "bad filepos"))?;
            entries.push((ms + offset_ms, pos));
        }
    }
    let palette = palette.ok_or_else(|| invalid(idx, "no palette"))?;
    let substream = 0x20 + stream.ok_or_else(|| invalid(idx, "no stream"))?;
    let units = entries
        .into_iter()
        .map(|(ms, pos)| {
            read_spu(&sub, pos, substream)
                .map(|spu| (ms, spu))
                .ok_or_else(|| invalid(&sub_path, &format!("no subpicture at {pos:#x}")))
        })
        .collect::<Result<_, _>>()?;
    Ok(VobSub {
        palette,
        language,
        units,
    })
}

/// `000000, f0f0f0, ...`: 16 RGB colours.
fn parse_palette(text: &str) -> Option<[[u8; 3]; 16]> {
    let mut palette = [[0u8; 3]; 16];
    let mut count = 0;
    for (slot, item) in palette.iter_mut().zip(text.split(',')) {
        let value = u32::from_str_radix(item.trim(), 16).ok()?;
        *slot = [(value >> 16) as u8, (value >> 8) as u8, value as u8];
        count += 1;
    }
    (count == 16).then_some(palette)
}

/// `hh:mm:ss:mmm` (also with a `-` in front), in ms.
fn parse_signed_ms(text: &str) -> Option<i64> {
    let text = text.trim();
    let (sign, text) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, text),
    };
    let parts: Vec<i64> = text
        .split(':')
        .map(|part| part.trim().parse().ok())
        .collect::<Option<_>>()?;
    let ms = match parts.as_slice() {
        [h, m, s, ms] => ((h * 60 + m) * 60 + s) * 1000 + ms,
        [ms] => *ms,
        _ => return None,
    };
    Some(sign * ms)
}

fn parse_language(text: &str) -> Option<[u8; 2]> {
    let bytes = text.as_bytes();
    match bytes {
        [a, b] if a.is_ascii_alphabetic() && b.is_ascii_alphabetic() => {
            Some([a.to_ascii_lowercase(), b.to_ascii_lowercase()])
        }
        _ => None,
    }
}

/// The subpicture unit of `substream` that starts in the pack at `pos` of the `.sub` (an MPEG program stream),
/// gathered from as many PES packets as it spans.
fn read_spu(sub: &[u8], pos: usize, substream: u8) -> Option<Vec<u8>> {
    let mut spu: Vec<u8> = Vec::new();
    let mut size: Option<usize> = None;
    let mut at = pos;
    while at + 4 <= sub.len() {
        if sub.get(at..at + 3)? != [0, 0, 1] {
            at += 1;
            continue;
        }
        let code = *sub.get(at + 3)?;
        match code {
            0xBA => {
                // Pack header: MPEG-2 (14 bytes + stuffing) or MPEG-1 (12 bytes).
                let mpeg2 = sub.get(at + 4)? & 0xC0 == 0x40;
                at += if mpeg2 {
                    14 + usize::from(sub.get(at + 13)? & 7)
                } else {
                    12
                };
            }
            0xB9 => return finish(spu, size),
            _ => {
                let length =
                    usize::from(u16::from_be_bytes([*sub.get(at + 4)?, *sub.get(at + 5)?]));
                let body = sub.get(at + 6..at + 6 + length)?;
                if code == 0xBD {
                    let header = usize::from(*body.get(2)?) + 3;
                    if body.get(header) == Some(&substream) {
                        let payload = body.get(header + 1..)?;
                        if size.is_none() {
                            size = Some(usize::from(u16::from_be_bytes([
                                *payload.first()?,
                                *payload.get(1)?,
                            ])));
                        }
                        spu.extend_from_slice(payload);
                        if size.is_some_and(|size| spu.len() >= size) {
                            return finish(spu, size);
                        }
                    }
                }
                at += 6 + length;
            }
        }
    }
    finish(spu, size)
}

fn finish(mut spu: Vec<u8>, size: Option<usize>) -> Option<Vec<u8>> {
    let size = size?;
    (spu.len() >= size).then(|| {
        spu.truncate(size);
        spu
    })
}

/// The colour indices a subpicture unit sets (its SET_COLOR commands, `0x03`), with where each is in the unit, so
/// they can be moved to other palette entries: (byte offset, high nibble?) for each of the four.
fn colour_positions(spu: &[u8]) -> Vec<usize> {
    let mut positions = Vec::new();
    let Some(mut dcsq) = spu
        .get(2..4)
        .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
    else {
        return positions;
    };
    let mut seen = Vec::new();
    while !seen.contains(&dcsq) && dcsq + 4 <= spu.len() {
        seen.push(dcsq);
        let next = usize::from(u16::from_be_bytes([spu[dcsq + 2], spu[dcsq + 3]]));
        let mut p = dcsq + 4;
        while let Some(&cmd) = spu.get(p) {
            p += 1;
            match cmd {
                0x00..=0x02 => {}
                0x03 => {
                    positions.push(p);
                    p += 2;
                }
                0x04 => p += 2,
                0x05 => p += 6,
                0x06 => p += 4,
                0x07 => {
                    let Some(len) = spu.get(p..p + 2) else { break };
                    p += usize::from(u16::from_be_bytes([len[0], len[1]]));
                }
                _ => break,
            }
        }
        dcsq = next;
    }
    positions
}

/// A title's palette (16 entries, `00 Y Cr Cb`) built from the colours its subtitles use: SRT tracks' fixed
/// entries, then each VobSub's colours, the same colour sharing an entry.
#[derive(Debug, Clone)]
pub(crate) struct PaletteBuilder {
    entries: Vec<u32>,
}

impl PaletteBuilder {
    /// With the entries SRT subtitles draw with (0 black, 1 white text, 2 black outline) when the title has any.
    pub(crate) fn new(with_text: bool) -> Self {
        let entries = if with_text {
            vec![0x0010_8080, 0x00EB_8080, 0x0010_8080]
        } else {
            Vec::new()
        };
        Self { entries }
    }

    /// The 16 entries (unused ones black).
    pub(crate) fn palette(&self) -> [u32; 16] {
        let mut palette = [0x0010_8080u32; 16];
        for (slot, entry) in palette.iter_mut().zip(&self.entries) {
            *slot = *entry;
        }
        palette
    }

    /// Moves `spu`'s colour indices (into `source`, a VobSub's RGB palette) to this palette's entries.
    ///
    /// # Errors
    ///
    /// When the title's subtitles use more than 16 colours together.
    pub(crate) fn remap(&mut self, spu: &mut [u8], source: &[[u8; 3]; 16]) -> Result<(), DvdError> {
        for pos in colour_positions(spu) {
            let Some(bytes) = spu.get_mut(pos..pos + 2) else {
                continue;
            };
            let nibbles = [bytes[0] >> 4, bytes[0] & 15, bytes[1] >> 4, bytes[1] & 15];
            let mut mapped = [0u8; 4];
            for (out, &index) in mapped.iter_mut().zip(&nibbles) {
                *out = self.entry_for(ycrcb(source[usize::from(index)]))?;
            }
            bytes[0] = (mapped[0] << 4) | mapped[1];
            bytes[1] = (mapped[2] << 4) | mapped[3];
        }
        Ok(())
    }

    fn entry_for(&mut self, colour: u32) -> Result<u8, DvdError> {
        let index = match self.entries.iter().position(|&entry| entry == colour) {
            Some(index) => index,
            None if self.entries.len() < 16 => {
                self.entries.push(colour);
                self.entries.len() - 1
            }
            None => {
                return Err(DvdError::Unsupported(
                    "the title's subtitles use more than the 16 colours a DVD title has"
                        .to_string(),
                ));
            }
        };
        u8::try_from(index).map_err(|_| DvdError::Invalid("palette index".to_string()))
    }
}

/// An RGB colour as a palette entry, `00 Y Cr Cb` (BT.601, video range).
fn ycrcb([r, g, b]: [u8; 3]) -> u32 {
    let (r, g, b) = (f64::from(r), f64::from(g), f64::from(b));
    let y = 16.0 + (65.481 * r + 128.553 * g + 24.966 * b) / 255.0;
    let cb = 128.0 + (-37.797 * r - 74.203 * g + 112.0 * b) / 255.0;
    let cr = 128.0 + (112.0 * r - 93.786 * g - 18.214 * b) / 255.0;
    let byte = |value: f64| value.round().clamp(0.0, 255.0) as u32;
    (byte(y) << 16) | (byte(cr) << 8) | byte(cb)
}

#[cfg(test)]
mod tests {
    use super::{PaletteBuilder, colour_positions, parse_palette, parse_signed_ms, read_spu};

    /// A minimal SPU: header, 4 bytes of "pixels", a DCSQ with SET_COLOR 0x1234, then end.
    fn spu() -> Vec<u8> {
        let mut spu = vec![0, 0, 0, 8, 0xAA, 0xBB, 0xCC, 0xDD];
        spu.extend_from_slice(&[0, 0, 0, 8, 0x03, 0x12, 0x34, 0xFF]);
        let size = spu.len() as u16;
        spu[0..2].copy_from_slice(&size.to_be_bytes());
        spu
    }

    #[test]
    fn idx_values_parse() {
        assert_eq!(parse_signed_ms("00:01:02:345"), Some(62_345));
        assert_eq!(parse_signed_ms("-00:00:00:500"), Some(-500));
        let palette = parse_palette(&["ff0000"; 16].join(", ")).unwrap();
        assert_eq!(palette[15], [255, 0, 0]);
        assert_eq!(parse_palette("ff0000, 00ff00"), None);
    }

    #[test]
    fn colours_move_to_the_title_palette_and_nothing_else_changes() {
        let mut unit = spu();
        assert_eq!(colour_positions(&unit), vec![13]);
        let mut source = [[0u8; 3]; 16];
        source[1] = [255, 255, 255];
        source[2] = [0, 0, 0];
        source[3] = [255, 0, 0];
        source[4] = [255, 255, 255];
        let mut builder = PaletteBuilder::new(true);
        builder.remap(&mut unit, &source).unwrap();
        // White and black are the SRT entries 1 and 0; red is new (3); the other white shares entry 1.
        assert_eq!(&unit[13..15], &[0x10, 0x31]);
        assert_eq!(&unit[..13], &spu()[..13]);
        assert_eq!(builder.palette()[3], super::ycrcb([255, 0, 0]));
    }

    #[test]
    fn a_unit_is_gathered_from_its_pes_packets() {
        let unit = spu();
        let (first, second) = unit.split_at(10);
        let mut sub = Vec::new();
        for chunk in [first, second] {
            // MPEG-2 pack header (14 bytes, no stuffing), then a private stream 1 PES with substream 0x20.
            sub.extend_from_slice(&[0, 0, 1, 0xBA, 0x44, 0, 4, 0, 4, 1, 1, 0x89, 0xC3, 0xF8]);
            let payload_len = (3 + 1 + chunk.len()) as u16;
            sub.extend_from_slice(&[0, 0, 1, 0xBD]);
            sub.extend_from_slice(&payload_len.to_be_bytes());
            sub.extend_from_slice(&[0x81, 0x00, 0x00, 0x20]);
            sub.extend_from_slice(chunk);
        }
        assert_eq!(read_spu(&sub, 0, 0x20), Some(unit));
        assert_eq!(read_spu(&sub, 0, 0x21), None);
    }
}
