//! The stream's parameters (RFC 9043 §4.2), its quantization table sets (§4.1) and the configuration record that holds
//! them for version 3 (§4.3), with its CRC (§4.9.3).

use crate::Error;
use crate::coder::{RangeDecoder, Transitions};

/// The most quantization table sets (§4.2.13).
const MOST_TABLE_SETS: usize = 8;
/// The most contexts of a table set (§4.1.2).
pub(crate) const MOST_CONTEXTS: usize = 32_768;
/// The adaptive states of a range-coded scalar (CONTEXT_SIZE).
pub(crate) const CONTEXT_SIZE: usize = 32;

/// One quantization table set: five tables of 256 entries, and how many contexts they make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableSet {
    pub(crate) tables: [[i32; 256]; 5],
    pub(crate) contexts: usize,
    /// The range coder's initial states of each context (all 128 when the stream does not give them).
    pub(crate) initial: Option<Vec<[u8; CONTEXT_SIZE]>>,
}

/// The parameters of a stream (version 3) or of the frames after a key frame (versions 0 and 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Parameters {
    pub(crate) version: u32,
    /// 0 Golomb-Rice, 1 range coder with the default table, 2 with a custom table.
    pub(crate) coder: u32,
    pub(crate) transitions: Transitions,
    /// 0 Y'CbCr, 1 RGB through the JPEG 2000 reversible colour transform.
    pub(crate) colorspace: u32,
    pub(crate) bits: u32,
    pub(crate) chroma_planes: bool,
    pub(crate) h_shift: u32,
    pub(crate) v_shift: u32,
    pub(crate) extra_plane: bool,
    pub(crate) h_slices: u32,
    pub(crate) v_slices: u32,
    pub(crate) sets: Vec<TableSet>,
    /// Each slice ends in a CRC (`ec` 1).
    pub(crate) slice_crc: bool,
}

impl Parameters {
    /// How many quantization table set indexes a slice header gives (§4.6.5).
    pub(crate) fn index_count(&self) -> usize {
        1 + usize::from(self.chroma_planes || self.version <= 3) + usize::from(self.extra_plane)
    }
}

/// Reads the parameters with the range decoder `coder` (their own states, all 128).
pub(crate) fn read_parameters(coder: &mut RangeDecoder<'_>) -> Result<Parameters, Error> {
    let tables = Transitions::default_tables();
    let mut state = [128u8; CONTEXT_SIZE];
    let unsigned =
        |coder: &mut RangeDecoder<'_>, state: &mut [u8; CONTEXT_SIZE]| -> Result<u32, Error> {
            let value = coder.symbol(state, false, &tables)?;
            u32::try_from(value).map_err(|_| Error::Corrupt("a negative parameter"))
        };
    let version = unsigned(coder, &mut state)?;
    if version == 2 || version > 3 {
        return Err(Error::Unsupported(format!("FFV1 version {version}")));
    }
    if version >= 3 {
        let _micro_version = unsigned(coder, &mut state)?;
    }
    let coder_type = unsigned(coder, &mut state)?;
    if coder_type > 2 {
        return Err(Error::Unsupported(format!("FFV1 coder type {coder_type}")));
    }
    let mut deltas = [0i32; 256];
    if coder_type > 1 {
        for delta in deltas.iter_mut().skip(1) {
            *delta = coder.symbol(&mut state, true, &tables)?;
        }
    }
    let transitions = Transitions::new(&deltas);
    let colorspace = unsigned(coder, &mut state)?;
    if colorspace > 1 {
        return Err(Error::Unsupported(format!(
            "FFV1 colour space {colorspace}"
        )));
    }
    let bits = if version >= 1 {
        unsigned(coder, &mut state)?
    } else {
        8
    };
    let bits = if bits == 0 { 8 } else { bits };
    if bits > 16 {
        return Err(Error::Unsupported(format!("FFV1 at {bits} bits")));
    }
    let chroma_planes = coder.bit(&mut state[0], &tables);
    let h_shift = unsigned(coder, &mut state)?;
    let v_shift = unsigned(coder, &mut state)?;
    if h_shift > 4 || v_shift > 4 {
        return Err(Error::Corrupt("a chroma subsampling past 16"));
    }
    let extra_plane = coder.bit(&mut state[0], &tables);
    let (mut h_slices, mut v_slices, mut set_count) = (1, 1, 1usize);
    if version >= 3 {
        h_slices = unsigned(coder, &mut state)?.saturating_add(1);
        v_slices = unsigned(coder, &mut state)?.saturating_add(1);
        set_count = usize::try_from(unsigned(coder, &mut state)?).unwrap_or(usize::MAX);
    }
    if set_count == 0 || set_count > MOST_TABLE_SETS {
        return Err(Error::Corrupt("a count of quantization table sets past 8"));
    }
    if h_slices > 1024 || v_slices > 1024 {
        return Err(Error::Corrupt("too many slices"));
    }
    let mut sets = Vec::with_capacity(set_count);
    for _ in 0..set_count {
        sets.push(read_table_set(coder, &tables)?);
    }
    let mut slice_crc = false;
    if version >= 3 {
        for set in &mut sets {
            if coder.bit(&mut state[0], &tables) {
                set.initial = Some(read_initial_states(coder, set.contexts, &tables)?);
            }
        }
        slice_crc = unsigned(coder, &mut state)? == 1;
        let _intra = unsigned(coder, &mut state)?;
    }
    if colorspace == 1 && (!chroma_planes || h_shift != 0 || v_shift != 0) {
        return Err(Error::Unsupported(
            "FFV1 RGB with subsampled or missing planes".to_string(),
        ));
    }
    Ok(Parameters {
        version,
        coder: coder_type,
        transitions,
        colorspace,
        bits,
        chroma_planes,
        h_shift,
        v_shift,
        extra_plane,
        h_slices,
        v_slices,
        sets,
        slice_crc,
    })
}

/// One quantization table set (§4.1): five tables, each given as runs of equal values over its first half, each
/// table with its own states (all 128; found against streams ffmpeg made: one set of states for the five tables
/// runs the second table past its 128 entries).
fn read_table_set(coder: &mut RangeDecoder<'_>, tables: &Transitions) -> Result<TableSet, Error> {
    let mut set = [[0i32; 256]; 5];
    let mut scale: i64 = 1;
    for table in &mut set {
        let mut state = [128u8; CONTEXT_SIZE];
        let mut value = 0i64;
        let mut k = 0usize;
        while k < 128 {
            let run = usize::try_from(coder.symbol(&mut state, false, tables)?)
                .map_err(|_| Error::Corrupt("a negative quantization run"))?
                .saturating_add(1);
            if k + run > 128 {
                return Err(Error::Corrupt("a quantization table past 128 entries"));
            }
            let entry = i32::try_from(scale * value)
                .map_err(|_| Error::Corrupt("a quantization value too large"))?;
            for slot in table.iter_mut().skip(k).take(run) {
                *slot = entry;
            }
            k += run;
            value += 1;
        }
        for k in 1..128 {
            let mirrored = -table.get(k).copied().unwrap_or(0);
            if let Some(slot) = table.get_mut(256 - k) {
                *slot = mirrored;
            }
        }
        table[128] = -table[127];
        scale *= 2 * value - 1;
        if scale > 2 * MOST_CONTEXTS as i64 {
            return Err(Error::Corrupt("too many contexts"));
        }
    }
    let contexts = usize::try_from((scale + 1) / 2).unwrap_or(0);
    if contexts == 0 || contexts > MOST_CONTEXTS {
        return Err(Error::Corrupt("too many contexts"));
    }
    Ok(TableSet {
        tables: set,
        contexts,
        initial: None,
    })
}

/// The initial states of each context of a table set (§4.2.15), each coded against the context before, with the
/// state's index as the range coder's context.
fn read_initial_states(
    coder: &mut RangeDecoder<'_>,
    contexts: usize,
    tables: &Transitions,
) -> Result<Vec<[u8; CONTEXT_SIZE]>, Error> {
    let mut states = vec![[128u8; CONTEXT_SIZE]; CONTEXT_SIZE];
    let mut initial: Vec<[u8; CONTEXT_SIZE]> = Vec::with_capacity(contexts);
    let mut before = [128u8; CONTEXT_SIZE];
    for _ in 0..contexts {
        let mut this = [0u8; CONTEXT_SIZE];
        for (k, slot) in this.iter_mut().enumerate() {
            let state = states.get_mut(k).ok_or(Error::Corrupt("a state index"))?;
            let delta = coder.symbol(state, true, tables)?;
            let predicted = i32::from(before.get(k).copied().unwrap_or(128));
            *slot = u8::try_from((predicted + delta) & 255).unwrap_or(0);
        }
        initial.push(this);
        before = this;
    }
    Ok(initial)
}

/// The parameters in a configuration record (§4.3); its CRC must leave no remainder.
pub(crate) fn read_configuration_record(record: &[u8]) -> Result<Parameters, Error> {
    if record.len() < 4 {
        return Err(Error::Corrupt("a configuration record too short"));
    }
    if crc(record) != 0 {
        return Err(Error::Corrupt(
            "the configuration record's CRC does not match",
        ));
    }
    let body = record.get(..record.len() - 4).unwrap_or_default();
    let mut coder = RangeDecoder::new(body);
    let parameters = read_parameters(&mut coder)?;
    if parameters.version < 3 {
        return Err(Error::Corrupt("a configuration record of version 0 or 1"));
    }
    Ok(parameters)
}

/// The CRC of `data` (§4.9.3): polynomial 0x04C11DB7, from 0, most significant bit first, no inversion.
pub(crate) fn crc(data: &[u8]) -> u32 {
    let mut crc = 0u32;
    for byte in data {
        crc ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04C1_1DB7
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::crc;

    /// A block followed by its own CRC (big-endian) leaves no remainder: what the records and slices rely on.
    #[test]
    fn a_block_with_its_crc_appended_leaves_no_remainder() {
        let mut block = b"FFV1 slice".to_vec();
        let parity = crc(&block);
        block.extend_from_slice(&parity.to_be_bytes());
        assert_eq!(crc(&block), 0);
        assert_ne!(crc(b"FFV1 slicf\0\0\0\0"), 0);
    }
}
