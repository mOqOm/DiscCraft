//! A picture's decoding (ITU-T H.263 §5 and §6, the baseline: I- and P-pictures of the five standard formats, none
//! of the optional modes of the annexes).

use crate::vlc::{BitReader, Event, Mcbpc, Vlc, cbpy, mcbpc_inter, mcbpc_intra, mvd, tcoef};
use crate::{Error, Picture};

/// The zigzag order (Figure 14): the block position (row * 8 + column) of each coefficient in transmission order.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// The code tables, built once per decoder.
pub(crate) struct Tables {
    mcbpc_intra: Vlc<Mcbpc>,
    mcbpc_inter: Vlc<Mcbpc>,
    cbpy: Vlc<u8>,
    mvd: Vlc<i8>,
    tcoef: Vlc<Event>,
    /// `C(u) / 2 * cos((2x + 1) u pi / 16)` for the inverse transform (§6.2.4).
    basis: [[f64; 8]; 8],
}

impl Tables {
    pub(crate) fn new() -> Self {
        let mut basis = [[0f64; 8]; 8];
        for (x, row) in basis.iter_mut().enumerate() {
            for (u, value) in row.iter_mut().enumerate() {
                let scale = if u == 0 {
                    std::f64::consts::FRAC_1_SQRT_2
                } else {
                    1.0
                };
                *value = scale / 2.0
                    * ((2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / 16.0).cos();
            }
        }
        Self {
            mcbpc_intra: mcbpc_intra(),
            mcbpc_inter: mcbpc_inter(),
            cbpy: cbpy(),
            mvd: mvd(),
            tcoef: tcoef(),
            basis,
        }
    }
}

/// A decoded picture's planes, each a whole number of macroblocks wide and high.
#[derive(Clone)]
pub(crate) struct Planes {
    pub(crate) y: Vec<u8>,
    pub(crate) cb: Vec<u8>,
    pub(crate) cr: Vec<u8>,
    /// Width and height in macroblocks.
    pub(crate) mbs: (usize, usize),
}

impl Planes {
    fn new(mbs: (usize, usize)) -> Self {
        let luma = mbs.0 * 16 * mbs.1 * 16;
        Self {
            y: vec![16; luma],
            cb: vec![128; luma / 4],
            cr: vec![128; luma / 4],
            mbs,
        }
    }
}

/// The source format of PTYPE bits 6 to 8 (Table 2): width and height.
fn source_format(code: u32) -> Result<(usize, usize), Error> {
    match code {
        1 => Ok((128, 96)),
        2 => Ok((176, 144)),
        3 => Ok((352, 288)),
        4 => Ok((704, 576)),
        5 => Ok((1408, 1152)),
        7 => Err(Error::Unsupported(
            "an extended picture type (H.263 version 2: custom sizes and the optional modes)"
                .to_string(),
        )),
        _ => Err(Error::Corrupt("a forbidden source format")),
    }
}

/// What the picture header says.
struct Header {
    size: (usize, usize),
    intra: bool,
    quant: i32,
    cpm: bool,
}

/// Finds the picture start code (allowing zero bytes before it) and reads the header (§5.1).
fn read_header(reader: &mut BitReader<'_>) -> Result<Header, Error> {
    // PSC: 0000 0000 0000 0000 1 00000 (22 bits), byte aligned.
    let mut found = false;
    for _ in 0..64 {
        if reader.peek(22) == 0b10_0000 {
            found = true;
            break;
        }
        if reader.peek(8) != 0 {
            break;
        }
        reader.skip(8);
    }
    if !found {
        return Err(Error::Corrupt("no picture start code"));
    }
    reader.skip(22);
    let _temporal_reference = reader.bits(8);
    if !reader.bit() || reader.bit() {
        return Err(Error::Corrupt("a picture type not of H.263"));
    }
    let _split_screen = reader.bit();
    let _document_camera = reader.bit();
    let _freeze_release = reader.bit();
    let size = source_format(reader.bits(3))?;
    let intra = !reader.bit();
    let annexes = reader.bits(4);
    if annexes != 0 {
        let names = [
            "unrestricted motion vectors (D)",
            "arithmetic coding (E)",
            "advanced prediction (F)",
            "PB-frames (G)",
        ];
        let used: Vec<&str> = names
            .iter()
            .enumerate()
            .filter(|(index, _)| annexes & (8 >> index) != 0)
            .map(|(_, name)| *name)
            .collect();
        return Err(Error::Unsupported(format!(
            "H.263 with {}",
            used.join(", ")
        )));
    }
    let quant = reader.bits(5) as i32;
    if quant == 0 {
        return Err(Error::Corrupt("a quantizer of 0"));
    }
    let cpm = reader.bit();
    if cpm {
        let _sub_bitstream = reader.bits(2);
    }
    // PEI and PSUPP.
    let mut extra = 0;
    while reader.bit() {
        reader.skip(8);
        extra += 1;
        if extra > 4096 {
            return Err(Error::Corrupt("supplemental information without end"));
        }
    }
    Ok(Header {
        size,
        intra,
        quant,
        cpm,
    })
}

/// Whether `data` (a picture) is an I-picture: its PTYPE bit 9, after the start code, the temporal reference and
/// PTYPE's first 8 bits.
pub(crate) fn is_intra(data: &[u8]) -> bool {
    let mut reader = BitReader::new(data);
    read_header(&mut reader).is_ok_and(|header| header.intra)
}

/// What a macroblock leaves for predicting the next ones' vectors.
#[derive(Clone, Copy, Default)]
struct MbInfo {
    /// The vector in half pixels, when the macroblock was INTER coded (INTRA and not coded ones predict 0).
    vector: Option<(i32, i32)>,
}

/// Decodes one picture with `reference` (the picture before, for a P-picture).
pub(crate) fn decode(
    tables: &Tables,
    data: &[u8],
    reference: Option<&Planes>,
) -> Result<(Picture, Planes), Error> {
    let mut reader = BitReader::new(data);
    let header = read_header(&mut reader)?;
    let (width, height) = header.size;
    let mbs = (width / 16, height / 16);
    let reference = match (header.intra, reference) {
        (true, _) => None,
        (false, Some(reference)) if reference.mbs == mbs => Some(reference),
        (false, _) => {
            return Err(Error::Corrupt(
                "a P-picture without a picture of its size before it",
            ));
        }
    };
    let mut planes = Planes::new(mbs);
    let mut info = vec![MbInfo::default(); mbs.0 * mbs.1];
    // Macroblock rows a GOB (Table 4).
    let rows_per_gob = if height <= 400 {
        1
    } else if height <= 800 {
        2
    } else {
        4
    };
    let mut quant = header.quant;
    for gob in 0..mbs.1 / rows_per_gob {
        let mut fresh_top = gob == 0;
        if gob > 0 && gob_start(&mut reader) {
            let number = reader.bits(5);
            if number == 31 {
                break;
            }
            if header.cpm {
                let _sub_bitstream = reader.bits(2);
            }
            let _frame_id = reader.bits(2);
            quant = reader.bits(5) as i32;
            if quant == 0 {
                return Err(Error::Corrupt("a quantizer of 0"));
            }
            fresh_top = true;
        }
        for row in 0..rows_per_gob {
            let mby = gob * rows_per_gob + row;
            for mbx in 0..mbs.0 {
                let above_outside = mby == 0 || (fresh_top && row == 0);
                decode_macroblock(
                    tables,
                    &mut reader,
                    &header,
                    &mut quant,
                    (mbx, mby),
                    above_outside,
                    &mut info,
                    &mut planes,
                    reference,
                )?;
                if reader.overrun() {
                    return Err(Error::Corrupt("a picture shorter than its macroblocks"));
                }
            }
        }
    }
    let picture = Picture {
        width: u32::try_from(width).unwrap_or(0),
        height: u32::try_from(height).unwrap_or(0),
        y: planes.y.clone(),
        cb: planes.cb.clone(),
        cr: planes.cr.clone(),
        intra: header.intra,
        pixel_aspect: (12, 11),
    };
    Ok((picture, planes))
}

/// Takes a GOB start code (GSTUF, then 0000 0000 0000 0000 1) when one follows, aligned or not.
fn gob_start(reader: &mut BitReader<'_>) -> bool {
    const GBSC: u32 = 1;
    if reader.peek(17) == GBSC {
        reader.skip(17);
        return true;
    }
    let position = reader.position();
    let stuffing = (8 - position % 8) % 8;
    if stuffing > 0 && reader.peek(stuffing as u32) == 0 {
        reader.skip(stuffing as u32);
        if reader.peek(17) == GBSC {
            reader.skip(17);
            return true;
        }
        reader.set_position(position);
    }
    false
}

/// Decodes the macroblock at (`mbx`, `mby`) into `planes`.
#[allow(clippy::too_many_arguments)]
fn decode_macroblock(
    tables: &Tables,
    reader: &mut BitReader<'_>,
    header: &Header,
    quant: &mut i32,
    (mbx, mby): (usize, usize),
    above_outside: bool,
    info: &mut [MbInfo],
    planes: &mut Planes,
    reference: Option<&Planes>,
) -> Result<(), Error> {
    let index = mby * planes.mbs.0 + mbx;
    // COD and MCBPC, past any stuffing.
    let (kind, cbpc) = loop {
        if !header.intra && reader.bit() {
            // Not coded: the reference's macroblock as it is.
            if let Some(reference) = reference {
                predict(planes, reference, (mbx, mby), (0, 0));
            }
            if let Some(slot) = info.get_mut(index) {
                *slot = MbInfo { vector: None };
            }
            return Ok(());
        }
        let table = if header.intra {
            &tables.mcbpc_intra
        } else {
            &tables.mcbpc_inter
        };
        match table
            .read(reader)
            .ok_or(Error::Corrupt("an unknown MCBPC code"))?
        {
            Mcbpc::Stuffing => {
                if reader.overrun() {
                    return Err(Error::Corrupt("stuffing past the end"));
                }
            }
            Mcbpc::Type { kind, cbpc } => break (kind, cbpc),
        }
    };
    if kind == 2 || kind == 5 {
        return Err(Error::Unsupported(
            "four motion vectors a macroblock (Annexes F and J)".to_string(),
        ));
    }
    let intra = kind == 3 || kind == 4;
    let pattern = tables
        .cbpy
        .read(reader)
        .ok_or(Error::Corrupt("an unknown CBPY code"))?;
    let cbpy = if intra { pattern } else { 15 - pattern };
    if kind == 1 || kind == 4 {
        let change = [-1, -2, 1, 2][reader.bits(2) as usize % 4];
        *quant = (*quant + change).clamp(1, 31);
    }
    let mut vector = (0, 0);
    if !intra {
        let predicted = predict_vector(info, planes.mbs, (mbx, mby), above_outside);
        let x = read_component(tables, reader, predicted.0)?;
        let y = read_component(tables, reader, predicted.1)?;
        vector = (x, y);
        match reference {
            Some(reference) => predict(planes, reference, (mbx, mby), vector),
            None => return Err(Error::Corrupt("an INTER macroblock in an I-picture")),
        }
    }
    if let Some(slot) = info.get_mut(index) {
        *slot = MbInfo {
            vector: (!intra).then_some(vector),
        };
    }
    let coded = (u32::from(cbpy) << 2) | u32::from(cbpc);
    for block in 0..6usize {
        let mut coefficients = [0i32; 64];
        let mut first = 0;
        if intra {
            let dc = reader.bits(8);
            coefficients[0] = match dc {
                0 | 128 => return Err(Error::Corrupt("a forbidden INTRADC code")),
                255 => 1024,
                value => value as i32 * 8,
            };
            first = 1;
        }
        let has_coefficients = coded & (1 << (5 - block)) != 0;
        if has_coefficients {
            read_coefficients(tables, reader, *quant, &mut coefficients, first)?;
        }
        if intra || has_coefficients {
            let residual = inverse_transform(&tables.basis, &coefficients);
            write_block(planes, (mbx, mby), block, &residual, intra);
        }
    }
    Ok(())
}

/// A vector component from its predictor and its MVD: the one of the code's two differences that keeps it in
/// [-16, 15.5] (§6.1.1), in half pixels.
fn read_component(
    tables: &Tables,
    reader: &mut BitReader<'_>,
    predicted: i32,
) -> Result<i32, Error> {
    let difference = i32::from(
        tables
            .mvd
            .read(reader)
            .ok_or(Error::Corrupt("an unknown MVD code"))?,
    );
    let mut value = predicted + difference;
    if value < -32 {
        value += 64;
    } else if value > 31 {
        value -= 64;
    }
    Ok(value)
}

/// The predictor of a macroblock's vector (§6.1.1, Figure 12): the median of the left, above and above-right
/// macroblocks' vectors, with the border rules in their order.
fn predict_vector(
    info: &[MbInfo],
    mbs: (usize, usize),
    (mbx, mby): (usize, usize),
    above_outside: bool,
) -> (i32, i32) {
    let vector = |x: usize, y: usize| {
        info.get(y * mbs.0 + x)
            .and_then(|mb| mb.vector)
            .unwrap_or((0, 0))
    };
    let mv1 = if mbx == 0 {
        (0, 0)
    } else {
        vector(mbx - 1, mby)
    };
    let (mv2, mut mv3) = if above_outside {
        (mv1, mv1)
    } else {
        (
            vector(mbx, mby - 1),
            if mbx + 1 < mbs.0 {
                vector(mbx + 1, mby - 1)
            } else {
                (0, 0)
            },
        )
    };
    if mbx + 1 >= mbs.0 {
        mv3 = (0, 0);
    }
    (median(mv1.0, mv2.0, mv3.0), median(mv1.1, mv2.1, mv3.1))
}

fn median(a: i32, b: i32, c: i32) -> i32 {
    a.max(b).min(a.min(b).max(c))
}

/// TCOEF events into `coefficients` from position `first` of the zigzag, inverse quantized (§6.2.1) and clipped
/// (§6.2.2).
fn read_coefficients(
    tables: &Tables,
    reader: &mut BitReader<'_>,
    quant: i32,
    coefficients: &mut [i32; 64],
    first: usize,
) -> Result<(), Error> {
    let mut position = first;
    loop {
        let (last, run, level) = match tables
            .tcoef
            .read(reader)
            .ok_or(Error::Corrupt("an unknown TCOEF code"))?
        {
            Event::Coefficient { last, run, level } => {
                let negative = reader.bit();
                let level = i32::from(level);
                (
                    last,
                    usize::from(run),
                    if negative { -level } else { level },
                )
            }
            Event::Escape => {
                let last = reader.bit();
                let run = reader.bits(6) as usize;
                let level = reader.bits(8) as u8 as i8;
                if level == 0 || level == i8::MIN {
                    return Err(Error::Corrupt("a forbidden escaped level"));
                }
                (last, run, i32::from(level))
            }
        };
        position += run;
        let slot = ZIGZAG
            .get(position)
            .and_then(|&at| coefficients.get_mut(at))
            .ok_or(Error::Corrupt("a coefficient past the block"))?;
        let magnitude = quant * (2 * level.abs() + 1) - i32::from(quant % 2 == 0);
        *slot = (level.signum() * magnitude).clamp(-2048, 2047);
        position += 1;
        if last {
            return Ok(());
        }
        if reader.overrun() {
            return Err(Error::Corrupt("a block past the end"));
        }
    }
}

/// The 8 x 8 inverse transform of §6.2.4 (rows then columns, in double precision: within Annex A's tolerance), its
/// output rounded and clipped to [-256, 255].
fn inverse_transform(basis: &[[f64; 8]; 8], coefficients: &[i32; 64]) -> [i32; 64] {
    let mut rows = [[0f64; 8]; 8];
    for v in 0..8 {
        for x in 0..8 {
            let mut sum = 0.0;
            for u in 0..8 {
                sum += basis[x][u] * f64::from(coefficients[v * 8 + u]);
            }
            rows[v][x] = sum;
        }
    }
    let mut out = [0i32; 64];
    for x in 0..8 {
        for y in 0..8 {
            let mut sum = 0.0;
            for v in 0..8 {
                sum += basis[y][v] * rows[v][x];
            }
            out[y * 8 + x] = (sum.round() as i32).clamp(-256, 255);
        }
    }
    out
}

/// The plane, its width and the block's place in it, for block `block` (Figure 5) of macroblock (`mbx`, `mby`).
fn block_place(
    planes: &mut Planes,
    (mbx, mby): (usize, usize),
    block: usize,
) -> (&mut Vec<u8>, usize, usize, usize) {
    let width = planes.mbs.0 * 16;
    match block {
        0..=3 => (
            &mut planes.y,
            width,
            mbx * 16 + (block % 2) * 8,
            mby * 16 + (block / 2) * 8,
        ),
        4 => (&mut planes.cb, width / 2, mbx * 8, mby * 8),
        _ => (&mut planes.cr, width / 2, mbx * 8, mby * 8),
    }
}

/// Writes `residual` as block `block`: the block itself (INTRA) or added to the prediction already there, clipped to
/// [0, 255] (§6.3).
fn write_block(
    planes: &mut Planes,
    mb: (usize, usize),
    block: usize,
    residual: &[i32; 64],
    intra: bool,
) {
    let (plane, width, x0, y0) = block_place(planes, mb, block);
    for y in 0..8 {
        for x in 0..8 {
            if let Some(sample) = plane.get_mut((y0 + y) * width + x0 + x) {
                let base = if intra { 0 } else { i32::from(*sample) };
                *sample = u8::try_from((base + residual[y * 8 + x]).clamp(0, 255)).unwrap_or(0);
            }
        }
    }
}

/// The motion-compensated prediction of macroblock (`mbx`, `mby`) from `reference` with `vector` (half pixels): the
/// luma with the vector, the chroma with it halved towards half pixels (Table 18), half pixels by bilinear
/// interpolation with rounding up (Figure 13; RCONTROL 0). Positions past the picture take its edge.
fn predict(
    planes: &mut Planes,
    reference: &Planes,
    (mbx, mby): (usize, usize),
    vector: (i32, i32),
) {
    let width = planes.mbs.0 * 16;
    let height = planes.mbs.1 * 16;
    copy_block(
        &reference.y,
        &mut planes.y,
        (width, height),
        (mbx * 16, mby * 16),
        16,
        vector,
    );
    let chroma = |v: i32| (v >> 1) | (v & 1);
    let chroma_vector = (chroma(vector.0), chroma(vector.1));
    copy_block(
        &reference.cb,
        &mut planes.cb,
        (width / 2, height / 2),
        (mbx * 8, mby * 8),
        8,
        chroma_vector,
    );
    copy_block(
        &reference.cr,
        &mut planes.cr,
        (width / 2, height / 2),
        (mbx * 8, mby * 8),
        8,
        chroma_vector,
    );
}

fn copy_block(
    from: &[u8],
    to: &mut [u8],
    (width, height): (usize, usize),
    (x0, y0): (usize, usize),
    size: usize,
    (vx, vy): (i32, i32),
) {
    let at = |x: i32, y: i32| -> i32 {
        let x = x.clamp(0, width as i32 - 1) as usize;
        let y = y.clamp(0, height as i32 - 1) as usize;
        i32::from(from.get(y * width + x).copied().unwrap_or(0))
    };
    let (fx, fy) = (vx & 1, vy & 1);
    for y in 0..size {
        for x in 0..size {
            let sx = (x0 + x) as i32 + (vx >> 1);
            let sy = (y0 + y) as i32 + (vy >> 1);
            let a = at(sx, sy);
            let value = match (fx, fy) {
                (0, 0) => a,
                (1, 0) => (a + at(sx + 1, sy) + 1) / 2,
                (0, _) => (a + at(sx, sy + 1) + 1) / 2,
                _ => (a + at(sx + 1, sy) + at(sx, sy + 1) + at(sx + 1, sy + 1) + 2) / 4,
            };
            if let Some(sample) = to.get_mut((y0 + y) * width + x0 + x) {
                *sample = u8::try_from(value.clamp(0, 255)).unwrap_or(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Tables, ZIGZAG, inverse_transform};

    /// The zigzag visits every position once.
    #[test]
    fn the_zigzag_is_a_permutation() {
        let mut seen = [false; 64];
        for at in ZIGZAG {
            assert!(!seen[at]);
            seen[at] = true;
        }
    }

    /// A DC coefficient alone gives a flat block of an eighth of it.
    #[test]
    fn a_dc_coefficient_gives_a_flat_block() {
        let tables = Tables::new();
        let mut coefficients = [0i32; 64];
        coefficients[0] = 800;
        let out = inverse_transform(&tables.basis, &coefficients);
        assert!(out.iter().all(|&value| value == 100));
    }
}
