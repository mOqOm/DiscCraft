use super::EsError;

/// MPEG-2 sequence header and extension information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceInfo {
    /// Encoded horizontal size in macroblock-aligned units.
    pub width: u16,
    /// Encoded vertical size in macroblock-aligned units.
    pub height: u16,
    /// Aspect ratio information code.
    pub aspect_code: u8,
    /// Frame rate code from the sequence header.
    pub frame_rate_code: u8,
    /// Bit rate value in units of 400 bits/s, including extension bits.
    pub bit_rate_400: u32,
    /// VBV buffer size value, including extension bits.
    pub vbv_buffer_size: u32,
    /// True when the sequence is progressive.
    pub progressive_sequence: bool,
    /// Profile and level from the sequence extension, or 0 if absent.
    pub profile_level: u8,
}

impl SequenceInfo {
    /// Whether the stream is MPEG-1 video (M6.350): its sequence header has no sequence extension, so no profile
    /// and level.
    #[must_use]
    pub fn is_mpeg1(&self) -> bool {
        self.profile_level == 0
    }
}

/// MPEG-2 picture coding type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureType {
    /// Intra-coded picture.
    I,
    /// Predictive-coded picture.
    P,
    /// Bidirectionally predictive-coded picture.
    B,
}

/// One MPEG-2 video access unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// Byte offset where this access unit starts in the elementary stream.
    pub offset: usize,
    /// Number of bytes until the next access unit start, or end of stream.
    pub len: usize,
    /// Picture coding type.
    pub kind: PictureType,
    /// 10-bit temporal reference.
    pub temporal_reference: u16,
    /// True when a GOP header precedes this picture.
    pub gop_start: bool,
    /// Value of closed_gop from the preceding GOP header.
    pub closed_gop: bool,
    /// True when a sequence header belongs to this access unit.
    pub has_sequence_header: bool,
    /// top_field_first from the picture coding extension.
    pub top_field_first: bool,
    /// repeat_first_field from the picture coding extension.
    pub repeat_first_field: bool,
    /// progressive_frame from the picture coding extension.
    pub progressive_frame: bool,
    /// Display duration in 27 MHz units.
    pub duration: i64,
    /// Presentation timestamp in 27 MHz units.
    pub pts: i64,
    /// Decode timestamp in 27 MHz units.
    pub dts: i64,
}

/// Parsed MPEG-2 video elementary stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoStream {
    /// Sequence information.
    pub sequence: SequenceInfo,
    /// Pictures in decode order.
    pub pictures: Vec<Picture>,
    /// True when a sequence end code was seen.
    pub has_sequence_end: bool,
}

const MAX_PICTURES: usize = 10_000_000;

/// Parse an MPEG-2 video elementary stream.
pub fn parse_video(data: &[u8], start_dts: i64) -> Result<VideoStream, EsError> {
    let mut parser = VideoParser::new();
    parser.feed(data, 0, true)?;
    parser.finish(data.len(), start_dts)
}

/// [`parse_video`] of the head of a stream (`data`, cut anywhere): a frame of two field pictures cut after its first
/// field is left out, and one cut inside its second field kept (M10.45), as the whole stream would have them whole.
///
/// # Errors
///
/// As [`parse_video`].
pub fn parse_video_head(data: &[u8], start_dts: i64) -> Result<VideoStream, EsError> {
    let mut parser = VideoParser::new();
    parser.feed(data, 0, true)?;
    parser.second_field = None;
    if parser.open_field.take().is_some()
        && let Some(cut) = parser.pictures.pop()
    {
        parser.current_picture = None;
        return parser.finish(cut.offset, start_dts);
    }
    parser.finish(data.len(), start_dts)
}

/// [`parse_video`] of a stream read from `reader` a chunk at a time, so it is never in memory whole (the pictures'
/// offsets are into the stream as read).
///
/// # Errors
///
/// As [`parse_video`], and when the stream cannot be read.
pub fn parse_video_reader(
    reader: impl std::io::Read,
    start_dts: i64,
) -> Result<VideoStream, EsError> {
    parse_video_chunked(reader, start_dts, 8 << 20)
}

/// [`parse_video_reader`] reading `chunk_len` bytes at a time.
fn parse_video_chunked(
    mut reader: impl std::io::Read,
    start_dts: i64,
    chunk_len: usize,
) -> Result<VideoStream, EsError> {
    let mut parser = VideoParser::new();
    let mut window: Vec<u8> = Vec::with_capacity(chunk_len + HEADER_WINDOW);
    let mut chunk = vec![0u8; chunk_len.max(1)];
    let mut base = 0usize;
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|error| es_err(&format!("read failed: {error}")))?;
        window.extend_from_slice(chunk.get(..read).unwrap_or_default());
        let last = read == 0;
        let consumed = parser.feed(&window, base, last)?;
        if last {
            return parser.finish(base + window.len(), start_dts);
        }
        window.drain(..consumed.min(window.len()));
        base += consumed;
    }
}

/// The most bytes after a start code that a header parsed here reads (a sequence header with both quantiser
/// matrices is 140): a start code is handled once that many bytes after it are in the chunk, or the stream has ended.
const HEADER_WINDOW: usize = 512;

/// Frame duration in 27 MHz units for MPEG-2 frame_rate_code values 1..=8.
pub fn frame_duration(frame_rate_code: u8) -> Option<i64> {
    match frame_rate_code {
        1 => Some(1_126_125),
        2 => Some(1_125_000),
        3 => Some(1_080_000),
        4 => Some(900_900),
        5 => Some(900_000),
        6 => Some(540_000),
        7 => Some(450_450),
        8 => Some(450_000),
        _ => None,
    }
}

fn es_err(msg: &str) -> EsError {
    EsError(msg.to_string())
}

struct BitReader<'a> {
    data: &'a [u8],
    byte: usize,
    bit: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8], byte: usize) -> Option<Self> {
        if byte > data.len() {
            None
        } else {
            Some(Self { data, byte, bit: 0 })
        }
    }

    fn read_bits(&mut self, n: u32) -> Option<u32> {
        if n > 32 {
            return None;
        }
        let mut value = 0u32;
        for _ in 0..n {
            let byte = self.data.get(self.byte)?;
            let mask = 1u8 << (7 - self.bit);
            let bit = if byte & mask != 0 { 1u32 } else { 0u32 };
            value = (value << 1) | bit;
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.byte = self.byte.checked_add(1)?;
            }
        }
        Some(value)
    }
}

fn reader_at(data: &[u8], offset: usize) -> Result<BitReader<'_>, EsError> {
    let start = offset
        .checked_add(4)
        .ok_or_else(|| es_err("start code offset overflow"))?;
    BitReader::new(data, start).ok_or_else(|| es_err("truncated start code"))
}

fn find_start_code(data: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while let Some(end) = i.checked_add(4) {
        if end > data.len() {
            break;
        }
        if data.get(i).copied() == Some(0)
            && data.get(i.checked_add(1)?).copied() == Some(0)
            && data.get(i.checked_add(2)?).copied() == Some(1)
        {
            return Some(i);
        }
        i = i.checked_add(1)?;
    }
    None
}

fn parse_sequence_header(data: &[u8], offset: usize) -> Result<SequenceInfo, EsError> {
    let mut br = reader_at(data, offset)?;
    let width = br
        .read_bits(12)
        .ok_or_else(|| es_err("truncated sequence header"))? as u16;
    let height = br
        .read_bits(12)
        .ok_or_else(|| es_err("truncated sequence header"))? as u16;
    let aspect_code = br
        .read_bits(4)
        .ok_or_else(|| es_err("truncated sequence header"))? as u8;
    let frame_rate_code = br
        .read_bits(4)
        .ok_or_else(|| es_err("truncated sequence header"))? as u8;
    let bit_rate_400 = br
        .read_bits(18)
        .ok_or_else(|| es_err("truncated sequence header"))?;
    let _marker = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated sequence header"))?;
    let vbv_buffer_size = br
        .read_bits(10)
        .ok_or_else(|| es_err("truncated sequence header"))?;

    Ok(SequenceInfo {
        width,
        height,
        aspect_code,
        frame_rate_code,
        bit_rate_400,
        vbv_buffer_size,
        progressive_sequence: false,
        profile_level: 0,
    })
}

fn read_extension_id(data: &[u8], offset: usize) -> Result<u32, EsError> {
    let mut br = reader_at(data, offset)?;
    br.read_bits(4).ok_or_else(|| es_err("truncated extension"))
}

fn parse_sequence_extension(
    data: &[u8],
    offset: usize,
    seq: &mut SequenceInfo,
) -> Result<(), EsError> {
    let mut br = reader_at(data, offset)?;
    let _id = br
        .read_bits(4)
        .ok_or_else(|| es_err("truncated sequence extension"))?;
    let profile_level = br
        .read_bits(8)
        .ok_or_else(|| es_err("truncated sequence extension"))? as u8;
    let progressive_sequence = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated sequence extension"))?
        != 0;
    let _chroma_format = br
        .read_bits(2)
        .ok_or_else(|| es_err("truncated sequence extension"))?;
    let _video_format = br
        .read_bits(3)
        .ok_or_else(|| es_err("truncated sequence extension"))?;
    let colour_description = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated sequence extension"))?;
    if colour_description != 0 {
        let _colour = br
            .read_bits(24)
            .ok_or_else(|| es_err("truncated sequence extension"))?;
    }
    let _display_horizontal_size = br
        .read_bits(14)
        .ok_or_else(|| es_err("truncated sequence extension"))?;
    let _display_vertical_size = br
        .read_bits(14)
        .ok_or_else(|| es_err("truncated sequence extension"))?;
    let bit_rate_extension = br
        .read_bits(12)
        .ok_or_else(|| es_err("truncated sequence extension"))?;
    let _marker = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated sequence extension"))?;
    let vbv_buffer_size_extension = br
        .read_bits(8)
        .ok_or_else(|| es_err("truncated sequence extension"))?;
    let _marker = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated sequence extension"))?;
    let _low_delay = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated sequence extension"))?;
    let _frame_rate_extension_n = br
        .read_bits(2)
        .ok_or_else(|| es_err("truncated sequence extension"))?;
    let _frame_rate_extension_d = br
        .read_bits(5)
        .ok_or_else(|| es_err("truncated sequence extension"))?;

    seq.profile_level = profile_level;
    seq.progressive_sequence = progressive_sequence;
    seq.bit_rate_400 = seq
        .bit_rate_400
        .checked_add(
            bit_rate_extension
                .checked_shl(18)
                .ok_or_else(|| es_err("bit rate extension overflow"))?,
        )
        .ok_or_else(|| es_err("bit rate overflow"))?;
    seq.vbv_buffer_size = seq
        .vbv_buffer_size
        .checked_add(
            vbv_buffer_size_extension
                .checked_shl(10)
                .ok_or_else(|| es_err("VBV extension overflow"))?,
        )
        .ok_or_else(|| es_err("VBV overflow"))?;
    Ok(())
}

/// What a picture coding extension says of how its picture is shown.
#[derive(Debug, Clone, Copy)]
struct CodingExtension {
    /// picture_structure: 1 the top field, 2 the bottom field, 3 a frame.
    structure: u8,
    top_field_first: bool,
    repeat_first_field: bool,
    progressive_frame: bool,
}

fn parse_picture_coding_extension(data: &[u8], offset: usize) -> Result<CodingExtension, EsError> {
    let mut br = reader_at(data, offset)?;
    let _id = br
        .read_bits(4)
        .ok_or_else(|| es_err("truncated picture coding extension"))?;
    let _f_codes = br
        .read_bits(16)
        .ok_or_else(|| es_err("truncated picture coding extension"))?;
    let _intra_dc_precision = br
        .read_bits(2)
        .ok_or_else(|| es_err("truncated picture coding extension"))?;
    let picture_structure = br
        .read_bits(2)
        .ok_or_else(|| es_err("truncated picture coding extension"))?;
    if picture_structure == 0 {
        return Err(es_err("invalid picture_structure"));
    }
    let top_field_first = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated picture coding extension"))?
        != 0;
    let _frame_pred_frame_dct = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated picture coding extension"))?;
    let _concealment_motion_vectors = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated picture coding extension"))?;
    let _q_scale_type = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated picture coding extension"))?;
    let _intra_vlc_format = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated picture coding extension"))?;
    let _alternate_scan = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated picture coding extension"))?;
    let repeat_first_field = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated picture coding extension"))?
        != 0;
    let _chroma_420_type = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated picture coding extension"))?;
    let progressive_frame = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated picture coding extension"))?
        != 0;
    let _composite_display_flag = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated picture coding extension"))?;

    Ok(CodingExtension {
        structure: u8::try_from(picture_structure).unwrap_or(3),
        top_field_first,
        repeat_first_field,
        progressive_frame,
    })
}

fn parse_gop_header(data: &[u8], offset: usize) -> Result<bool, EsError> {
    let mut br = reader_at(data, offset)?;
    // time_code is 25 bits and already includes drop_frame_flag (13818-2 §6.2.2.6).
    let _time_code = br
        .read_bits(25)
        .ok_or_else(|| es_err("truncated GOP header"))?;
    let closed_gop = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated GOP header"))?
        != 0;
    let _broken_link = br
        .read_bits(1)
        .ok_or_else(|| es_err("truncated GOP header"))?;
    Ok(closed_gop)
}

fn parse_picture_header(data: &[u8], offset: usize) -> Result<(u16, PictureType), EsError> {
    let mut br = reader_at(data, offset)?;
    let temporal_reference = br
        .read_bits(10)
        .ok_or_else(|| es_err("truncated picture header"))? as u16;
    let coding_type = br
        .read_bits(3)
        .ok_or_else(|| es_err("truncated picture header"))?;
    let kind = match coding_type {
        1 => PictureType::I,
        2 => PictureType::P,
        3 => PictureType::B,
        _ => return Err(es_err("invalid picture coding type")),
    };
    Ok((temporal_reference, kind))
}

fn picture_duration(_progressive_sequence: bool, repeat_first_field: bool, frame_dur: i64) -> i64 {
    if repeat_first_field {
        frame_dur.saturating_mul(3).saturating_div(2)
    } else {
        frame_dur
    }
}

fn add_gop_order(
    order: &mut Vec<usize>,
    pictures: &[Picture],
    start: usize,
    end: usize,
) -> Result<(), EsError> {
    let mut idx: Vec<usize> = Vec::with_capacity(end.saturating_sub(start));
    for i in start..end {
        idx.push(i);
    }

    idx.sort_by_key(|&i| match pictures.get(i) {
        Some(p) => p.temporal_reference,
        None => 0,
    });

    let mut split = None;
    for pair in idx.windows(2) {
        let a = pair
            .first()
            .copied()
            .ok_or_else(|| es_err("missing picture index"))?;
        let b = pair
            .get(1)
            .copied()
            .ok_or_else(|| es_err("missing picture index"))?;
        let tr_a = pictures
            .get(a)
            .ok_or_else(|| es_err("missing picture"))?
            .temporal_reference;
        let tr_b = pictures
            .get(b)
            .ok_or_else(|| es_err("missing picture"))?
            .temporal_reference;
        if tr_b.wrapping_sub(tr_a) > 512 {
            split = Some(b);
        }
    }

    if let Some(s) = split
        && let Some(pos) = idx.iter().position(|&i| i == s)
    {
        idx.rotate_left(pos);
    }

    order.extend(idx);
    Ok(())
}

struct VideoParser {
    /// A start code has been handled (the first must be a sequence header).
    started: bool,
    seq: Option<SequenceInfo>,
    pictures: Vec<Picture>,
    has_sequence_end: bool,
    pending_start: Option<usize>,
    pending_has_sequence: bool,
    pending_gop: Option<bool>,
    current_picture: Option<usize>,
    first_picture: bool,
    /// The first field of a frame coded as two field pictures, waiting for its second (13818-2 6.1.1.4).
    open_field: Option<OpenField>,
    /// The second field's picture header has been read; its coding extension comes next.
    second_field: Option<OpenField>,
}

/// A frame's first field picture: its picture_structure, temporal_reference and type.
#[derive(Debug, Clone, Copy)]
struct OpenField {
    structure: u8,
    temporal_reference: u16,
    kind: PictureType,
}

impl VideoParser {
    fn new() -> Self {
        Self {
            started: false,
            seq: None,
            pictures: Vec::new(),
            has_sequence_end: false,
            pending_start: None,
            pending_has_sequence: false,
            pending_gop: None,
            current_picture: None,
            first_picture: true,
            open_field: None,
            second_field: None,
        }
    }

    /// Handles the start codes in `data` (the stream from byte `base`) whose headers it holds whole: all of them
    /// when it is the `last` of the stream. Returns how many of its bytes are done with; the rest come again at the
    /// front of the next chunk.
    fn feed(&mut self, data: &[u8], base: usize, last: bool) -> Result<usize, EsError> {
        let limit = if last {
            data.len()
        } else {
            data.len().saturating_sub(HEADER_WINDOW)
        };
        let mut pos = 0usize;
        while let Some(sc) = find_start_code(data, pos) {
            if sc >= limit {
                break;
            }
            let code_offset = sc
                .checked_add(3)
                .ok_or_else(|| es_err("start code offset overflow"))?;
            let code = data
                .get(code_offset)
                .copied()
                .ok_or_else(|| es_err("missing start code"))?;
            if !self.started {
                if code != 0xB3 {
                    return Err(es_err("stream must start with sequence header"));
                }
                self.started = true;
            }
            self.handle_start_code(data, base, sc, code)?;
            pos = sc.checked_add(1).ok_or_else(|| es_err("offset overflow"))?;
            if self.pictures.len() >= MAX_PICTURES {
                return Err(es_err("too many pictures"));
            }
        }
        Ok(limit)
    }

    fn handle_start_code(
        &mut self,
        data: &[u8],
        base: usize,
        sc: usize,
        code: u8,
    ) -> Result<(), EsError> {
        let at = base
            .checked_add(sc)
            .ok_or_else(|| es_err("offset overflow"))?;
        match code {
            0xB3 => self.handle_sequence(data, sc, at),
            0xB5 => self.handle_extension(data, sc),
            0xB8 => self.handle_gop(data, sc, at),
            0x00 => self.handle_picture(data, sc, at),
            0xB7 => {
                self.has_sequence_end = true;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// `sc` is the start code's offset in `data`, `at` in the stream.
    fn handle_sequence(&mut self, data: &[u8], sc: usize, at: usize) -> Result<(), EsError> {
        let info = parse_sequence_header(data, sc)?;
        self.seq = Some(info);
        if self.pending_start.is_none() {
            self.pending_start = Some(at);
        }
        self.pending_has_sequence = true;
        self.current_picture = None;
        Ok(())
    }

    fn handle_gop(&mut self, data: &[u8], sc: usize, at: usize) -> Result<(), EsError> {
        let closed = parse_gop_header(data, sc)?;
        if self.pending_start.is_none() {
            self.pending_start = Some(at);
        }
        self.pending_gop = Some(closed);
        self.current_picture = None;
        Ok(())
    }

    fn handle_picture(&mut self, data: &[u8], sc: usize, at: usize) -> Result<(), EsError> {
        if self.seq.is_none() {
            return Err(es_err("missing sequence header"));
        }
        if self.pictures.len() >= MAX_PICTURES {
            return Err(es_err("too many pictures"));
        }

        let (temporal_reference, kind) = parse_picture_header(data, sc)?;
        if self.second_field.is_some() {
            return Err(es_err("a field picture without its coding extension"));
        }
        // M10.45: the second field picture of a frame belongs to its first's access unit (the same
        // temporal_reference, nothing between them; after an I or P field an I or P one, after a B field a B one).
        if let Some(open) = self.open_field.take() {
            let pairs = open.temporal_reference == temporal_reference
                && self.pending_start.is_none()
                && (open.kind == PictureType::B) == (kind == PictureType::B);
            if !pairs {
                return Err(es_err("a field picture without its second field"));
            }
            self.second_field = Some(open);
            self.current_picture = None;
            return Ok(());
        }
        let offset = match self.pending_start {
            Some(o) => o,
            None => at,
        };
        let gop_start = self.pending_gop.is_some();
        let closed_gop = self.pending_gop.unwrap_or_default();
        let has_sequence_header = self.pending_has_sequence;

        if self.first_picture
            && (kind != PictureType::I || !gop_start || !closed_gop || !has_sequence_header)
        {
            return Err(es_err("first picture must be an I picture in a closed GOP"));
        }

        let idx = self.pictures.len();
        self.pictures.push(Picture {
            offset,
            len: 0,
            kind,
            temporal_reference,
            gop_start,
            closed_gop,
            has_sequence_header,
            top_field_first: false,
            repeat_first_field: false,
            progressive_frame: false,
            duration: 0,
            pts: 0,
            dts: 0,
        });

        self.current_picture = Some(idx);
        self.pending_start = None;
        self.pending_has_sequence = false;
        self.pending_gop = None;
        self.first_picture = false;
        Ok(())
    }

    fn handle_extension(&mut self, data: &[u8], sc: usize) -> Result<(), EsError> {
        let id = read_extension_id(data, sc)?;
        match id {
            1 => {
                if let Some(seq) = self.seq.as_mut() {
                    parse_sequence_extension(data, sc, seq)?;
                } else {
                    return Err(es_err("sequence extension before sequence header"));
                }
            }
            8 => {
                let extension = parse_picture_coding_extension(data, sc)?;
                if let Some(open) = self.second_field.take() {
                    // The other field of the frame.
                    if extension.structure == 3 || extension.structure == open.structure {
                        return Err(es_err("a field picture without its second field"));
                    }
                    return Ok(());
                }
                let idx = self
                    .current_picture
                    .ok_or_else(|| es_err("picture coding extension without picture"))?;
                let pic = self
                    .pictures
                    .get_mut(idx)
                    .ok_or_else(|| es_err("missing picture"))?;
                if extension.structure == 3 {
                    pic.top_field_first = extension.top_field_first;
                    pic.repeat_first_field = extension.repeat_first_field;
                    pic.progressive_frame = extension.progressive_frame;
                } else {
                    // A frame as two field pictures: shown in the order they come, the first one's field first.
                    pic.top_field_first = extension.structure == 1;
                    pic.repeat_first_field = false;
                    pic.progressive_frame = false;
                    self.open_field = Some(OpenField {
                        structure: extension.structure,
                        temporal_reference: pic.temporal_reference,
                        kind: pic.kind,
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// The parsed stream, `total` bytes long.
    fn finish(&mut self, total: usize, start_dts: i64) -> Result<VideoStream, EsError> {
        if !self.started {
            return Err(es_err("no start code"));
        }
        let seq = self
            .seq
            .clone()
            .ok_or_else(|| es_err("missing sequence header"))?;
        if self.pictures.is_empty() {
            return Err(es_err("no pictures"));
        }
        if self.open_field.is_some() || self.second_field.is_some() {
            return Err(es_err("a field picture without its second field"));
        }

        self.set_lengths(total)?;
        let frame_dur =
            frame_duration(seq.frame_rate_code).ok_or_else(|| es_err("invalid frame rate code"))?;
        self.set_durations(&seq, frame_dur);
        self.set_timestamps(start_dts, frame_dur)?;

        let pictures = std::mem::take(&mut self.pictures);
        Ok(VideoStream {
            sequence: seq,
            pictures,
            has_sequence_end: self.has_sequence_end,
        })
    }

    fn set_lengths(&mut self, total: usize) -> Result<(), EsError> {
        let len = self.pictures.len();
        for i in 0..len {
            let offset = self
                .pictures
                .get(i)
                .ok_or_else(|| es_err("missing picture"))?
                .offset;
            let next = if let Some(next_idx) = i.checked_add(1) {
                if next_idx < len {
                    self.pictures
                        .get(next_idx)
                        .ok_or_else(|| es_err("missing picture"))?
                        .offset
                } else {
                    total
                }
            } else {
                total
            };
            let unit_len = next
                .checked_sub(offset)
                .ok_or_else(|| es_err("picture offsets out of order"))?;
            if let Some(pic) = self.pictures.get_mut(i) {
                pic.len = unit_len;
            }
        }
        Ok(())
    }

    fn set_durations(&mut self, seq: &SequenceInfo, frame_dur: i64) {
        for pic in self.pictures.iter_mut() {
            pic.duration =
                picture_duration(seq.progressive_sequence, pic.repeat_first_field, frame_dur);
        }
    }

    fn set_timestamps(&mut self, start_dts: i64, frame_dur: i64) -> Result<(), EsError> {
        let has_b = self.pictures.iter().any(|p| p.kind == PictureType::B);

        let mut acc = start_dts;
        for pic in self.pictures.iter_mut() {
            pic.dts = acc;
            acc = acc
                .checked_add(pic.duration)
                .ok_or_else(|| es_err("dts overflow"))?;
        }

        let display = self.display_order()?;
        let mut acc = start_dts;
        for idx in display {
            if let Some(pic) = self.pictures.get_mut(idx) {
                pic.pts = acc;
                acc = acc
                    .checked_add(pic.duration)
                    .ok_or_else(|| es_err("pts overflow"))?;
            } else {
                return Err(es_err("missing picture"));
            }
        }

        // The reorder delay: one frame with B pictures, or more when repeat_first_field (soft pulldown) makes the
        // pictures shown ahead of an anchor last longer than it, and never so little that a picture is shown before
        // it is decoded.
        let needed = self
            .pictures
            .iter()
            .map(|pic| pic.dts.saturating_sub(pic.pts))
            .max()
            .unwrap_or(0);
        let delay = if has_b {
            needed.max(frame_dur)
        } else {
            needed.max(0)
        };
        for pic in self.pictures.iter_mut() {
            pic.pts = pic
                .pts
                .checked_add(delay)
                .ok_or_else(|| es_err("pts overflow"))?;
        }

        for pic in self.pictures.iter() {
            if pic.pts < pic.dts {
                return Err(es_err("invalid picture order"));
            }
        }
        Ok(())
    }

    fn display_order(&self) -> Result<Vec<usize>, EsError> {
        let mut order = Vec::with_capacity(self.pictures.len());
        let mut start = 0;
        for i in 1..self.pictures.len() {
            if self
                .pictures
                .get(i)
                .ok_or_else(|| es_err("missing picture"))?
                .gop_start
            {
                add_gop_order(&mut order, &self.pictures, start, i)?;
                start = i;
            }
        }
        add_gop_order(&mut order, &self.pictures, start, self.pictures.len())?;
        if order.len() != self.pictures.len() {
            return Err(es_err("invalid picture order"));
        }
        Ok(order)
    }
}

#[cfg(test)]
mod tests {
    use super::{PictureType, frame_duration, parse_video, parse_video_head};

    struct BitWriter {
        bytes: Vec<u8>,
        current: u8,
        bits_left: u8,
    }

    impl BitWriter {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                current: 0,
                bits_left: 8,
            }
        }

        fn write_bits(&mut self, value: u32, n: u32) {
            for i in (0..n).rev() {
                let bit = ((value >> i) & 1) as u8;
                self.current |= bit << (self.bits_left - 1);
                self.bits_left -= 1;
                if self.bits_left == 0 {
                    self.bytes.push(self.current);
                    self.current = 0;
                    self.bits_left = 8;
                }
            }
        }

        fn finish(&mut self) {
            if self.bits_left != 8 {
                self.bytes.push(self.current);
                self.current = 0;
                self.bits_left = 8;
            }
        }
    }

    fn append_start_code(bytes: &mut Vec<u8>, code: u8) {
        bytes.extend_from_slice(&[0, 0, 1, code]);
    }

    fn write_sequence_header(bytes: &mut Vec<u8>) {
        append_start_code(bytes, 0xB3);
        let mut bw = BitWriter::new();
        bw.write_bits(720, 12);
        bw.write_bits(480, 12);
        bw.write_bits(3, 4);
        bw.write_bits(4, 4);
        bw.write_bits(10000, 18);
        bw.write_bits(1, 1);
        bw.write_bits(1000, 10);
        bw.write_bits(1, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.finish();
        bytes.extend_from_slice(&bw.bytes);
    }

    fn write_sequence_extension(bytes: &mut Vec<u8>, progressive: u32) {
        append_start_code(bytes, 0xB5);
        let mut bw = BitWriter::new();
        bw.write_bits(1, 4);
        bw.write_bits(0x45, 8);
        bw.write_bits(progressive, 1);
        bw.write_bits(1, 2);
        bw.write_bits(0, 3);
        bw.write_bits(0, 1);
        bw.write_bits(720, 14);
        bw.write_bits(480, 14);
        bw.write_bits(0, 12);
        bw.write_bits(1, 1);
        bw.write_bits(0, 8);
        bw.write_bits(1, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 2);
        bw.write_bits(0, 5);
        bw.finish();
        bytes.extend_from_slice(&bw.bytes);
    }

    fn write_gop(bytes: &mut Vec<u8>, closed: u32) {
        append_start_code(bytes, 0xB8);
        let mut bw = BitWriter::new();
        bw.write_bits(0, 25); // time_code incl. drop_frame_flag
        bw.write_bits(closed, 1);
        bw.write_bits(0, 1);
        bw.finish();
        bytes.extend_from_slice(&bw.bytes);
    }

    fn write_picture_header(bytes: &mut Vec<u8>, tr: u32, coding: u32) {
        append_start_code(bytes, 0x00);
        let mut bw = BitWriter::new();
        bw.write_bits(tr, 10);
        bw.write_bits(coding, 3);
        bw.write_bits(0, 16);
        if coding == 2 || coding == 3 {
            bw.write_bits(0, 1);
            bw.write_bits(0, 3);
        }
        if coding == 3 {
            bw.write_bits(0, 1);
            bw.write_bits(0, 3);
        }
        bw.finish();
        bytes.extend_from_slice(&bw.bytes);
    }

    fn write_picture_coding_extension(bytes: &mut Vec<u8>, structure: u32, tff: u32, rff: u32) {
        append_start_code(bytes, 0xB5);
        let mut bw = BitWriter::new();
        bw.write_bits(8, 4);
        bw.write_bits(0, 16);
        bw.write_bits(0, 2);
        bw.write_bits(structure, 2);
        bw.write_bits(tff, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.write_bits(rff, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.write_bits(0, 1);
        bw.finish();
        bytes.extend_from_slice(&bw.bytes);
    }

    fn write_slice(bytes: &mut Vec<u8>) {
        append_start_code(bytes, 0x01);
    }

    fn build_valid_video() -> Vec<u8> {
        let mut data = Vec::new();
        write_sequence_header(&mut data);
        write_sequence_extension(&mut data, 0);
        write_gop(&mut data, 1);

        write_picture_header(&mut data, 0, 1);
        write_picture_coding_extension(&mut data, 3, 1, 0);
        write_slice(&mut data);

        write_picture_header(&mut data, 3, 2);
        write_picture_coding_extension(&mut data, 3, 1, 0);
        write_slice(&mut data);

        write_picture_header(&mut data, 1, 3);
        write_picture_coding_extension(&mut data, 3, 1, 0);
        write_slice(&mut data);

        write_picture_header(&mut data, 2, 3);
        write_picture_coding_extension(&mut data, 3, 1, 0);
        write_slice(&mut data);

        data
    }

    #[test]
    fn frame_duration_values() {
        assert_eq!(frame_duration(4), Some(900_900));
        assert_eq!(frame_duration(3), Some(1_080_000));
        assert_eq!(frame_duration(0), None);
    }

    #[test]
    fn a_stream_read_in_chunks_parses_as_one_slice() {
        // Many pictures, read a few bytes at a time and in chunks larger than the header window.
        let mut data = build_valid_video();
        let one = data.clone();
        for _ in 0..200 {
            data.extend_from_slice(&one);
        }
        let whole = parse_video(&data, 1_000_000).map_err(|err| err.0);
        for chunk in [1, 7, 100, 513, 4096, data.len()] {
            let chunked =
                super::parse_video_chunked(&data[..], 1_000_000, chunk).map_err(|err| err.0);
            assert_eq!(chunked, whole, "chunks of {chunk}");
        }
        assert!(whole.is_ok());
    }

    #[test]
    #[allow(clippy::cognitive_complexity)] // one end-to-end assertion list
    fn parses_basic_video() {
        let data = build_valid_video();
        let start = 1_000_000;
        let result = parse_video(&data, start);
        assert!(result.is_ok());

        if let Ok(stream) = result {
            assert_eq!(stream.pictures.len(), 4);
            assert_eq!(stream.sequence.profile_level, 0x45);
            assert!(!stream.sequence.progressive_sequence);

            assert_eq!(stream.pictures[0].kind, PictureType::I);
            assert_eq!(stream.pictures[1].kind, PictureType::P);
            assert_eq!(stream.pictures[2].kind, PictureType::B);
            assert_eq!(stream.pictures[3].kind, PictureType::B);

            assert_eq!(stream.pictures[0].temporal_reference, 0);
            assert_eq!(stream.pictures[1].temporal_reference, 3);
            assert_eq!(stream.pictures[2].temporal_reference, 1);
            assert_eq!(stream.pictures[3].temporal_reference, 2);

            assert!(stream.pictures[0].gop_start);
            assert!(stream.pictures[0].closed_gop);
            assert!(stream.pictures[0].has_sequence_header);
            assert!(!stream.pictures[1].gop_start);
            assert!(!stream.pictures[2].gop_start);
            assert!(!stream.pictures[3].gop_start);

            let frame = 900_900;
            assert_eq!(stream.pictures[0].dts, start);
            assert_eq!(stream.pictures[1].dts, start + frame);
            assert_eq!(stream.pictures[2].dts, start + 2 * frame);
            assert_eq!(stream.pictures[3].dts, start + 3 * frame);

            assert_eq!(stream.pictures[0].pts, start + frame);
            assert_eq!(stream.pictures[1].pts, start + 4 * frame);
            assert_eq!(stream.pictures[2].pts, start + 2 * frame);
            assert_eq!(stream.pictures[3].pts, start + 3 * frame);

            assert_eq!(stream.pictures[0].offset, 0);
            assert_eq!(stream.pictures[0].len, stream.pictures[1].offset);
            assert_eq!(
                stream.pictures[1].len,
                stream.pictures[2].offset - stream.pictures[1].offset
            );
            assert_eq!(
                stream.pictures[2].len,
                stream.pictures[3].offset - stream.pictures[2].offset
            );
            assert_eq!(
                stream.pictures[3].offset + stream.pictures[3].len,
                data.len()
            );
        }
    }

    #[test]
    fn rejects_missing_sequence_header() {
        let mut data = Vec::new();
        write_gop(&mut data, 1);
        write_picture_header(&mut data, 0, 1);
        write_picture_coding_extension(&mut data, 3, 1, 0);
        assert!(parse_video(&data, 0).is_err());
    }

    #[test]
    fn rejects_a_field_picture_without_its_second() {
        let mut data = Vec::new();
        write_sequence_header(&mut data);
        write_sequence_extension(&mut data, 0);
        write_gop(&mut data, 1);
        write_picture_header(&mut data, 0, 1);
        write_picture_coding_extension(&mut data, 1, 1, 0);
        assert!(parse_video(&data, 0).is_err());
    }

    /// Frames of two field pictures (top, then bottom): I + P, P + P, B + B. Returns the stream and where each
    /// frame starts.
    fn build_field_video() -> (Vec<u8>, Vec<usize>) {
        let mut data = Vec::new();
        write_sequence_header(&mut data);
        write_sequence_extension(&mut data, 0);
        write_gop(&mut data, 1);
        let mut starts = vec![0];
        for (index, (tr, first, second)) in [(0, 1, 2), (3, 2, 2), (1, 3, 3), (2, 3, 3)]
            .into_iter()
            .enumerate()
        {
            if index > 0 {
                starts.push(data.len());
            }
            for (coding, structure) in [(first, 1), (second, 2)] {
                write_picture_header(&mut data, tr, coding);
                // A field picture's top_field_first is 0 (13818-2 6.3.10).
                write_picture_coding_extension(&mut data, structure, 0, 0);
                write_slice(&mut data);
            }
        }
        (data, starts)
    }

    /// M10.45: a frame of two field pictures is one access unit, of the first field's type, shown for a frame,
    /// its top field first.
    #[test]
    fn field_pictures_pair_into_frames() {
        let (data, starts) = build_field_video();
        let parsed = parse_video(&data, 0).unwrap();
        let kinds: Vec<PictureType> = parsed.pictures.iter().map(|picture| picture.kind).collect();
        assert_eq!(
            kinds,
            [
                PictureType::I,
                PictureType::P,
                PictureType::B,
                PictureType::B
            ]
        );
        let offsets: Vec<usize> = parsed
            .pictures
            .iter()
            .map(|picture| picture.offset)
            .collect();
        assert_eq!(offsets, starts);
        assert_eq!(parsed.pictures[0].len, starts[1]);
        for picture in &parsed.pictures {
            assert!(
                picture.top_field_first
                    && !picture.progressive_frame
                    && !picture.repeat_first_field
            );
            assert_eq!(picture.duration, 900_900);
        }
    }

    #[test]
    fn rejects_a_second_field_that_does_not_pair() {
        for (second_type, second_structure) in [(3, 2), (2, 1), (2, 3)] {
            let mut data = Vec::new();
            write_sequence_header(&mut data);
            write_sequence_extension(&mut data, 0);
            write_gop(&mut data, 1);
            write_picture_header(&mut data, 0, 1);
            write_picture_coding_extension(&mut data, 1, 0, 0);
            write_slice(&mut data);
            write_picture_header(&mut data, 0, second_type);
            write_picture_coding_extension(&mut data, second_structure, 0, 0);
            write_slice(&mut data);
            assert!(
                parse_video(&data, 0).is_err(),
                "{second_type} {second_structure}"
            );
        }
    }

    /// The head of a stream cut after a frame's first field: that frame is left out.
    #[test]
    fn a_head_cut_inside_a_frame_of_fields_leaves_it_out() {
        let (data, starts) = build_field_video();
        // Cut where the second frame's second field starts.
        let second_field = (starts[1] + 4..data.len())
            .find(|&at| data.get(at..at + 4) == Some(&[0, 0, 1, 0][..]))
            .unwrap_or(data.len());
        let cut = data.get(..second_field).unwrap_or(&data);
        assert!(parse_video(cut, 0).is_err());
        let head = parse_video_head(cut, 0).unwrap();
        assert_eq!(head.pictures.len(), 1);
        assert_eq!(head.pictures[0].len, starts[1]);
        for i in 0..=data.len() {
            let _ = parse_video_head(data.get(..i).unwrap_or(&data), 0);
        }
    }

    #[test]
    fn rejects_truncated_header() {
        let data = build_valid_video();
        let truncated = data.get(..5).unwrap_or(&data);
        assert!(parse_video(truncated, 0).is_err());
    }

    #[test]
    fn prefixes_do_not_panic() {
        let data = build_valid_video();
        for i in 0..=data.len() {
            let prefix = data.get(..i).unwrap_or(&data);
            let _ = parse_video(prefix, 0);
        }
    }
}
