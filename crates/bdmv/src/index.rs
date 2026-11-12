//! BDMV/index.bdmv reading and writing.

use crate::{BdmvError, Reader, VERSION, len_u32, patch_u32};

const APPINFO_LEN: u32 = 34;
const TITLE_LEN: usize = 12;
const INDEXES_FIXED_LEN: usize = 26;
const HEADER_RESERVED_LEN: usize = 24;
const USER_DATA_LEN: usize = 32;

/// One HDMV title entry: which movie object runs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexTitle {
    /// Movie object id to run.
    pub movie_object: u16,
    /// Whether the title is HDMV interactive.
    pub interactive: bool,
}

/// The index table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexTable {
    /// Movie object id run at disc insertion.
    pub first_playback: u16,
    /// Movie object id of the top menu.
    pub top_menu: u16,
    /// Titles after first playback and top menu.
    pub titles: Vec<IndexTitle>,
    /// AppInfoBDMV VideoFormat code.
    pub video_format: u8,
    /// AppInfoBDMV FrameRate code.
    pub frame_rate: u8,
}

struct BitWriter {
    buf: Vec<u8>,
    current: u8,
    bits: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            current: 0,
            bits: 0,
        }
    }

    fn write_bits(&mut self, value: u32, count: u8) -> Result<(), BdmvError> {
        if count == 0 {
            return Ok(());
        }
        if count > 32 {
            return Err(BdmvError("bit count too large".to_string()));
        }
        if count < 32 {
            let limit = 1u32 << u32::from(count);
            if value >= limit {
                return Err(BdmvError(format!(
                    "value {value} does not fit in {count} bits"
                )));
            }
        }

        for shift in (0..count).rev() {
            let bit = ((value >> u32::from(shift)) & 1) == 1;
            self.current = self
                .current
                .checked_shl(1)
                .ok_or_else(|| BdmvError("bit writer overflow".to_string()))?
                | u8::from(bit);
            self.bits = self
                .bits
                .checked_add(1)
                .ok_or_else(|| BdmvError("bit writer overflow".to_string()))?;

            if self.bits == 8 {
                self.buf.push(self.current);
                self.current = 0;
                self.bits = 0;
            }
        }

        Ok(())
    }

    fn finish(mut self) -> Result<Vec<u8>, BdmvError> {
        if self.bits != 0 {
            let shift = 8u8
                .checked_sub(self.bits)
                .ok_or_else(|| BdmvError("bit writer padding overflow".to_string()))?;
            self.current = self
                .current
                .checked_shl(u32::from(shift))
                .ok_or_else(|| BdmvError("bit writer padding overflow".to_string()))?;
            self.buf.push(self.current);
        }
        Ok(self.buf)
    }
}

fn write_title(out: &mut Vec<u8>, movie_object: u16, playback: u8) -> Result<(), BdmvError> {
    if playback > 1 {
        return Err(BdmvError("invalid HDMV playback type".to_string()));
    }

    let mut bw = BitWriter::new();
    bw.write_bits(1, 2)?; // ObjectType: HDMV.
    bw.write_bits(0, 2)?; // AccessType or reserved for first/top titles.
    bw.write_bits(0, 28)?; // reserved.
    bw.write_bits(u32::from(playback), 2)?; // PlaybackType.
    bw.write_bits(0, 14)?; // reserved.
    bw.write_bits(u32::from(movie_object), 16)?; // RefToMovieObjectID.
    bw.write_bits(0, 32)?; // reserved.

    out.extend_from_slice(&bw.finish()?);
    Ok(())
}

fn write_app_info(out: &mut Vec<u8>, video_format: u8, frame_rate: u8) -> Result<(), BdmvError> {
    out.extend_from_slice(&APPINFO_LEN.to_be_bytes());

    let mut bw = BitWriter::new();
    bw.write_bits(0, 1)?; // reserved.
    bw.write_bits(0, 1)?; // InitialOutputModePreference: 2D.
    bw.write_bits(0, 1)?; // SSContentExistFlag: absent.
    bw.write_bits(0, 1)?; // reserved.
    bw.write_bits(0, 4)?; // InitialDynamicRangeType.
    bw.write_bits(u32::from(video_format), 4)?;
    bw.write_bits(u32::from(frame_rate), 4)?;

    out.extend_from_slice(&bw.finish()?);
    out.extend_from_slice(&[0u8; USER_DATA_LEN]);
    Ok(())
}

fn indexes_content_len(titles_len: usize) -> Result<usize, BdmvError> {
    let titles_bytes = titles_len
        .checked_mul(TITLE_LEN)
        .ok_or_else(|| BdmvError("index titles length overflow".to_string()))?;
    INDEXES_FIXED_LEN
        .checked_add(titles_bytes)
        .ok_or_else(|| BdmvError("index length overflow".to_string()))
}

fn write_indexes(out: &mut Vec<u8>, table: &IndexTable) -> Result<(), BdmvError> {
    let count =
        u16::try_from(table.titles.len()).map_err(|_| BdmvError("too many titles".to_string()))?;
    let content_len = indexes_content_len(table.titles.len())?;

    out.extend_from_slice(&len_u32(content_len)?.to_be_bytes());
    write_title(out, table.first_playback, 0)?;
    write_title(out, table.top_menu, 1)?;
    out.extend_from_slice(&count.to_be_bytes());

    for title in &table.titles {
        write_title(out, title.movie_object, u8::from(title.interactive))?;
    }

    Ok(())
}

/// Write an `index.bdmv` file.
pub fn write_index(table: &IndexTable) -> Result<Vec<u8>, BdmvError> {
    let mut out = Vec::new();

    out.extend_from_slice(b"INDX");
    out.extend_from_slice(VERSION);
    out.extend_from_slice(&0u32.to_be_bytes()); // IndexesStartAddress placeholder.
    out.extend_from_slice(&0u32.to_be_bytes()); // ExtensionDataStartAddress.
    out.extend_from_slice(&[0u8; HEADER_RESERVED_LEN]);

    write_app_info(&mut out, table.video_format, table.frame_rate)?;

    let indexes_start = len_u32(out.len())?;
    patch_u32(&mut out, 8, indexes_start)?;

    write_indexes(&mut out, table)?;
    Ok(out)
}

fn read_header(r: &mut Reader<'_>) -> Result<usize, BdmvError> {
    if !r.bytes(4)?.starts_with(b"INDX") {
        return Err(BdmvError("bad index type indicator".to_string()));
    }
    if !r.bytes(4)?.starts_with(VERSION) {
        return Err(BdmvError("bad index version".to_string()));
    }

    let indexes_start = r.u32()?;
    if r.u32()? != 0 {
        return Err(BdmvError(
            "index extension data is not supported".to_string(),
        ));
    }
    r.skip(HEADER_RESERVED_LEN)?;

    usize::try_from(indexes_start).map_err(|_| BdmvError("index start out of range".to_string()))
}

fn read_app_info(r: &mut Reader<'_>) -> Result<(u8, u8), BdmvError> {
    if r.u32()? != APPINFO_LEN {
        return Err(BdmvError("bad AppInfoBDMV length".to_string()));
    }

    r.skip(1)?;
    let second = r.u8()?;
    r.skip(USER_DATA_LEN)?;

    Ok(((second >> 4) & 0x0F, second & 0x0F))
}

fn title_value(bytes: &[u8]) -> Result<u128, BdmvError> {
    let mut value = 0u128;
    for b in bytes {
        value = value
            .checked_shl(8)
            .ok_or_else(|| BdmvError("title value overflow".to_string()))?
            | u128::from(*b);
    }
    Ok(value)
}

fn title_field(value: u128, shift: u32, mask: u128) -> Result<u32, BdmvError> {
    let shifted = value
        .checked_shr(shift)
        .ok_or_else(|| BdmvError("title field shift overflow".to_string()))?;
    u32::try_from(shifted & mask).map_err(|_| BdmvError("title field too large".to_string()))
}

fn read_title(r: &mut Reader<'_>) -> Result<IndexTitle, BdmvError> {
    let value = title_value(r.bytes(TITLE_LEN)?)?;

    let object_type = u8::try_from(title_field(value, 94, 3)?)
        .map_err(|_| BdmvError("bad object type".to_string()))?;
    if object_type != 1 {
        return Err(BdmvError("only HDMV titles are supported".to_string()));
    }

    let playback = title_field(value, 62, 3)?;
    if playback > 1 {
        return Err(BdmvError("bad HDMV playback type".to_string()));
    }

    let movie_object = u16::try_from(title_field(value, 32, 0xFFFFu128)?)
        .map_err(|_| BdmvError("bad movie object id".to_string()))?;

    Ok(IndexTitle {
        movie_object,
        interactive: playback != 0,
    })
}

fn read_titles(r: &mut Reader<'_>) -> Result<Vec<IndexTitle>, BdmvError> {
    let count = usize::from(r.u16()?);
    let mut titles = Vec::with_capacity(count);
    for _ in 0..count {
        titles.push(read_title(r)?);
    }
    Ok(titles)
}

fn read_indexes_length(r: &mut Reader<'_>) -> Result<usize, BdmvError> {
    let length_pos = r.pos();
    let length = usize::try_from(r.u32()?)
        .map_err(|_| BdmvError("index length out of range".to_string()))?;
    length_pos
        .checked_add(4)
        .and_then(|x| x.checked_add(length))
        .ok_or_else(|| BdmvError("index length overflow".to_string()))
}

fn check_end(r: &Reader<'_>, expected_end: usize) -> Result<(), BdmvError> {
    if r.pos() != expected_end {
        return Err(BdmvError("index object length mismatch".to_string()));
    }
    Ok(())
}

fn read_indexes(r: &mut Reader<'_>) -> Result<(u16, u16, Vec<IndexTitle>), BdmvError> {
    let expected_end = read_indexes_length(r)?;
    let first_playback = read_title(r)?.movie_object;
    let top_menu = read_title(r)?.movie_object;
    let titles = read_titles(r)?;
    check_end(r, expected_end)?;
    Ok((first_playback, top_menu, titles))
}

/// Read an `index.bdmv` file.
pub fn read_index(data: &[u8]) -> Result<IndexTable, BdmvError> {
    let mut r = Reader::at(data, 0);
    let indexes_start = read_header(&mut r)?;
    let (video_format, frame_rate) = read_app_info(&mut r)?;

    if indexes_start != r.pos() {
        return Err(BdmvError("indexes start mismatch".to_string()));
    }

    let mut r = Reader::at(data, indexes_start);
    let (first_playback, top_menu, titles) = read_indexes(&mut r)?;

    Ok(IndexTable {
        first_playback,
        top_menu,
        titles,
        video_format,
        frame_rate,
    })
}

#[cfg(test)]
mod tests {
    use super::{IndexTable, IndexTitle, read_index, write_index};

    fn sample() -> IndexTable {
        IndexTable {
            first_playback: 0,
            top_menu: 1,
            titles: vec![
                IndexTitle {
                    movie_object: 2,
                    interactive: false,
                },
                IndexTitle {
                    movie_object: 3,
                    interactive: true,
                },
                IndexTitle {
                    movie_object: 4,
                    interactive: false,
                },
            ],
            video_format: 6,
            frame_rate: 4,
        }
    }

    #[test]
    fn round_trip() -> Result<(), crate::BdmvError> {
        let data = write_index(&sample())?;
        let back = read_index(&data)?;
        assert_eq!(back, sample());
        Ok(())
    }

    #[test]
    fn header() -> Result<(), crate::BdmvError> {
        let data = write_index(&sample())?;
        assert!(data.starts_with(b"INDX0200"));
        Ok(())
    }

    #[test]
    fn truncated() -> Result<(), crate::BdmvError> {
        let mut data = write_index(&sample())?;
        let _ = data.pop();
        assert!(read_index(&data).is_err());
        Ok(())
    }
}
