//! BD LPCM access-unit framing.

use crate::{EsUnit, M2tsError};

const PTS_PER_UNIT: u64 = 450;

/// Split big-endian 16-bit 48 kHz samples of one or two channels into 5 ms BD LPCM units.
pub fn bd_lpcm_units(
    samples: &[u8],
    channels: u8,
    start_pts: u64,
) -> Result<Vec<EsUnit>, M2tsError> {
    bd_lpcm_units_of(samples, channels, 0, (16, 48_000), start_pts)
}

/// The most a BD LPCM stream carries, bits a second (six channels of 24 bits at 192 kHz), counting its channels
/// as stored.
const MAX_BITRATE: u64 = 27_648_000;

/// WAV speaker positions (`dwChannelMask` bits): front left, right and centre, low frequency, back left and
/// right, back centre, side left and right.
const FL: u32 = 0x1;
const FR: u32 = 0x2;
const FC: u32 = 0x4;
const LFE: u32 = 0x8;
const BL: u32 = 0x10;
const BR: u32 = 0x20;
const BC: u32 = 0x100;
const SL: u32 = 0x200;
const SR: u32 = 0x400;

/// The BD LPCM channel assignments of more than two channels (the code in a unit's header) with their channels
/// in the stream's order, as WAV speakers: 3/0 (L R C), 2/1 (L R S), 3/1 (L R C S), 2/2 (L R Ls Rs), 3/2
/// (L R C Ls Rs), 3/2 with LFE (L R C Ls Rs LFE), 3/4 (L R C Ls Rls Rrs Rs) and 3/4 with LFE. A layout's two
/// surrounds are a WAV's back or side ones; of four, the side ones are the outer pair. The order is as ffmpeg
/// and VLC decode it (black-box, M6.347).
const LAYOUTS: [(u8, &[u32]); 11] = [
    (4, &[FL, FR, FC]),
    (5, &[FL, FR, BC]),
    (6, &[FL, FR, FC, BC]),
    (7, &[FL, FR, BL, BR]),
    (7, &[FL, FR, SL, SR]),
    (8, &[FL, FR, FC, BL, BR]),
    (8, &[FL, FR, FC, SL, SR]),
    (9, &[FL, FR, FC, BL, BR, LFE]),
    (9, &[FL, FR, FC, SL, SR, LFE]),
    (10, &[FL, FR, FC, SL, BL, BR, SR]),
    (11, &[FL, FR, FC, SL, BL, BR, SR, LFE]),
];

/// The speakers of a WAV's `channels` when the file names none: the usual ones of that count (seven: 6.1, M6.544).
fn usual_speakers(channels: u8) -> Option<u32> {
    match channels {
        3 => Some(FL | FR | FC),
        4 => Some(FL | FR | BL | BR),
        5 => Some(FL | FR | FC | BL | BR),
        6 => Some(FL | FR | FC | LFE | BL | BR),
        7 => Some(FL | FR | FC | LFE | BC | SL | SR),
        8 => Some(FL | FR | FC | LFE | BL | BR | SL | SR),
        _ => None,
    }
}

/// Where a channel of a BD LPCM stream comes from (M6.544).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LpcmSource {
    /// The WAV's channel, as it is.
    Channel(usize),
    /// The WAV's channel 3 dB down (one channel shared by two speakers).
    Halved(usize),
    /// Silence (a speaker the WAV has no channel for).
    Silent,
}

/// Speakers the assignments do not have, put on one that does (M6.544): 2.1, 3.1, 2/1 and 3/1 with LFE and 4.1 on
/// 3/2 with LFE (9), the missing speakers silent and a single surround shared by both, 3 dB down; 6.1 on 3/4 with
/// LFE (11), its back centre shared by both back speakers, 3 dB down. Each: the WAV's speakers, the code, and each
/// stream channel's speaker (0: silent) with whether it is shared.
const EXTRA_LAYOUTS: [(u32, u8, &[(u32, bool)]); 8] = [
    (
        FL | FR | LFE,
        9,
        &[
            (FL, false),
            (FR, false),
            (0, false),
            (0, false),
            (0, false),
            (LFE, false),
        ],
    ),
    (
        FL | FR | FC | LFE,
        9,
        &[
            (FL, false),
            (FR, false),
            (FC, false),
            (0, false),
            (0, false),
            (LFE, false),
        ],
    ),
    (
        FL | FR | BC | LFE,
        9,
        &[
            (FL, false),
            (FR, false),
            (0, false),
            (BC, true),
            (BC, true),
            (LFE, false),
        ],
    ),
    (
        FL | FR | FC | BC | LFE,
        9,
        &[
            (FL, false),
            (FR, false),
            (FC, false),
            (BC, true),
            (BC, true),
            (LFE, false),
        ],
    ),
    (
        FL | FR | BL | BR | LFE,
        9,
        &[
            (FL, false),
            (FR, false),
            (0, false),
            (BL, false),
            (BR, false),
            (LFE, false),
        ],
    ),
    (
        FL | FR | SL | SR | LFE,
        9,
        &[
            (FL, false),
            (FR, false),
            (0, false),
            (SL, false),
            (SR, false),
            (LFE, false),
        ],
    ),
    (
        FL | FR | FC | LFE | BC | SL | SR,
        11,
        &[
            (FL, false),
            (FR, false),
            (FC, false),
            (SL, false),
            (BC, true),
            (BC, true),
            (SR, false),
            (LFE, false),
        ],
    ),
    (
        FL | FR | FC | LFE | BL | BR | BC,
        11,
        &[
            (FL, false),
            (FR, false),
            (FC, false),
            (BL, false),
            (BC, true),
            (BC, true),
            (BR, false),
            (LFE, false),
        ],
    ),
];

/// The WAV channel of speaker `speaker` among `mask`'s (a WAV's channels are in the order of their speakers' bits).
fn wav_channel(mask: u32, speaker: u32) -> Option<usize> {
    usize::try_from((mask & speaker.wrapping_sub(1)).count_ones()).ok()
}

/// The BD LPCM channel assignment of a WAV's `channels` on speakers `mask` (its `dwChannelMask`; 0, or a mask of
/// another count, for the usual ones), and for each channel of the stream, in the stream's order, where it comes
/// from. One channel is mono and two stereo whatever their speakers. Speakers no assignment has (2.1, 4.1, 6.1 …)
/// go on a larger one ([`EXTRA_LAYOUTS`], M6.544). `None` for others.
#[must_use]
pub fn bd_lpcm_layout(channels: u8, mask: u32) -> Option<(u8, Vec<LpcmSource>)> {
    match channels {
        1 => return Some((1, vec![LpcmSource::Channel(0)])),
        2 => {
            return Some((3, vec![LpcmSource::Channel(0), LpcmSource::Channel(1)]));
        }
        _ => {}
    }
    let mask = if mask.count_ones() == u32::from(channels) {
        mask
    } else {
        usual_speakers(channels)?
    };
    if let Some((code, speakers)) = LAYOUTS
        .iter()
        .find(|(_, speakers)| speakers.iter().fold(0, |all, speaker| all | speaker) == mask)
    {
        let order = speakers
            .iter()
            .map(|speaker| wav_channel(mask, *speaker).map(LpcmSource::Channel))
            .collect::<Option<Vec<_>>>()?;
        return Some((*code, order));
    }
    let (_, code, slots) = EXTRA_LAYOUTS
        .iter()
        .find(|(speakers, _, _)| *speakers == mask)?;
    let order = slots
        .iter()
        .map(|(speaker, shared)| match (*speaker, *shared) {
            (0, _) => Some(LpcmSource::Silent),
            (speaker, false) => wav_channel(mask, speaker).map(LpcmSource::Channel),
            (speaker, true) => wav_channel(mask, speaker).map(LpcmSource::Halved),
        })
        .collect::<Option<Vec<_>>>()?;
    Some((*code, order))
}

/// The channels a BD LPCM stream of a WAV's `channels` on speakers `mask` stores (an even count; M6.544: a layout
/// put on a larger assignment stores its channels); `None` when it has no assignment.
#[must_use]
pub fn bd_lpcm_stored(channels: u8, mask: u32) -> Option<u8> {
    let (_, order) = bd_lpcm_layout(channels, mask)?;
    u8::try_from(order.len().max(2).next_multiple_of(2)).ok()
}

/// Appends the big-endian signed sample `sample` (2 or 3 bytes) 3 dB down (× 0.7071, rounded).
fn push_halved(out: &mut Vec<u8>, sample: &[u8]) {
    let value = sample
        .iter()
        .fold(0i32, |value, byte| (value << 8) | i32::from(*byte));
    // Sign-extend from the sample's width.
    let shift = 32 - 8 * i32::try_from(sample.len()).unwrap_or(4);
    let value = (value << shift) >> shift;
    // `as` after rounding a value inside the sample's range.
    let scaled = (f64::from(value) * std::f64::consts::FRAC_1_SQRT_2).round() as i32;
    let bytes = scaled.to_be_bytes();
    out.extend_from_slice(bytes.get(4 - sample.len()..).unwrap_or_default());
}

/// What a BD LPCM unit's header says (M6.366: the sound of a transport stream is read back as a WAV).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BdLpcmFormat {
    /// The channels the sound has.
    pub channels: u8,
    /// The channels stored a sample frame: an even count (a mono stream stores two).
    pub stored: u8,
    /// Samples a second: 48, 96 or 192 kHz.
    pub rate: u32,
    /// Bytes a stored sample: 2 (16 bits) or 3 (20 and 24 bits).
    pub sample_bytes: u8,
    /// The bits a sample says it holds: 16, 20 or 24.
    pub bits: u8,
    /// The WAV speakers (`dwChannelMask`) of the channels; 0 for mono and stereo.
    pub mask: u32,
    /// For each channel of the stream, in its order, the WAV channel it is.
    pub wav_order: Vec<usize>,
}

/// Reads the four-byte header of a BD LPCM unit (the inverse of what [`bd_lpcm_units_of`] writes); `None` for a
/// channel assignment, rate or sample size the format does not have.
#[must_use]
pub fn read_bd_lpcm_header(header: &[u8]) -> Option<BdLpcmFormat> {
    let (third, fourth) = (*header.get(2)?, *header.get(3)?);
    let rate = match third & 0x0F {
        1 => 48_000,
        4 => 96_000,
        5 => 192_000,
        _ => return None,
    };
    let (bits, sample_bytes) = match fourth >> 6 {
        1 => (16, 2),
        2 => (20, 3),
        3 => (24, 3),
        _ => return None,
    };
    let (channels, mask, wav_order) = match third >> 4 {
        1 => (1, 0, vec![0]),
        3 => (2, 0, vec![0, 1]),
        code => {
            let (_, speakers) = LAYOUTS.iter().find(|(found, _)| *found == code)?;
            let mask = speakers.iter().fold(0, |all, speaker| all | speaker);
            // A WAV's channels are in the order of their speakers' bits.
            let order = speakers
                .iter()
                .map(|speaker| usize::try_from((mask & speaker.wrapping_sub(1)).count_ones()).ok())
                .collect::<Option<Vec<_>>>()?;
            (u8::try_from(speakers.len()).ok()?, mask, order)
        }
    };
    Some(BdLpcmFormat {
        channels,
        stored: channels.max(2).next_multiple_of(2),
        rate,
        sample_bytes,
        bits,
        mask,
        wav_order,
    })
}

/// Split big-endian interleaved samples of `bits` (16 or 24) at `sample_rate` (48, 96 or 192 kHz) of `channels`
/// on WAV speakers `mask` (see [`bd_lpcm_layout`]) into 5 ms BD LPCM units: 240 sample frames at 48 kHz, 480 at
/// 96 kHz, 960 at 192 kHz. The channels go in the stream's order; an odd count is stored with one more, silent
/// (one channel as two), as the stream holds an even number of channels.
pub fn bd_lpcm_units_of(
    samples: &[u8],
    channels: u8,
    mask: u32,
    (bits, sample_rate): (u8, u32),
    start_pts: u64,
) -> Result<Vec<EsUnit>, M2tsError> {
    let (assignment, order) = bd_lpcm_layout(channels, mask).ok_or_else(|| {
        M2tsError("BD LPCM has no channel assignment for these speakers".to_string())
    })?;
    let (frames_per_unit, rate_code) = match sample_rate {
        48_000 => (240_usize, 1_u8),
        96_000 => (480, 4),
        192_000 => (960, 5),
        _ => {
            return Err(M2tsError("BD LPCM is 48, 96 or 192 kHz".to_string()));
        }
    };
    let (sample_bytes, bits_code) = match bits {
        16 => (2_usize, 1_u8),
        24 => (3, 3),
        _ => return Err(M2tsError("BD LPCM is 16 or 24 bits".to_string())),
    };
    let stored = order.len().max(2).next_multiple_of(2);
    let stored_bits = u64::from(sample_rate) * u64::from(bits) * stored as u64;
    if stored_bits > MAX_BITRATE {
        return Err(M2tsError("BD LPCM is at most 27.648 Mbit/s".to_string()));
    }
    // What a unit holds: its frames of the stored channels.
    let unit_payload = frames_per_unit * stored * sample_bytes;

    let channel_count = usize::from(channels);
    let frame_bytes = channel_count * sample_bytes;

    if !samples.len().is_multiple_of(frame_bytes) {
        return Err(M2tsError(
            "BD LPCM sample byte count is not a whole frame".to_string(),
        ));
    }

    let total_frames = samples.len() / frame_bytes;
    let unit_count = total_frames.div_ceil(frames_per_unit);
    let header = bd_header(assignment, unit_payload, rate_code, bits_code)?;
    // Channels in the WAV's order need no reordering.
    let in_order = order.len() == channel_count
        && order
            .iter()
            .enumerate()
            .all(|(slot, source)| *source == LpcmSource::Channel(slot));
    let silent = vec![0_u8; (stored - order.len()) * sample_bytes];

    let mut units = Vec::with_capacity(unit_count);

    for index in 0..unit_count {
        let start_frame = index * frames_per_unit;
        let frames = (total_frames - start_frame).min(frames_per_unit);
        let offset = start_frame * frame_bytes;
        let len = frames * frame_bytes;

        let chunk = samples
            .get(offset..offset.saturating_add(len))
            .unwrap_or_default();

        let mut data = Vec::with_capacity(4 + unit_payload);
        data.extend_from_slice(&header);
        if in_order && silent.is_empty() {
            data.extend_from_slice(chunk);
        } else {
            for frame in chunk.chunks_exact(frame_bytes) {
                for source in &order {
                    let sample = |channel: usize| {
                        let at = channel * sample_bytes;
                        frame.get(at..at + sample_bytes).unwrap_or_default()
                    };
                    match *source {
                        LpcmSource::Channel(channel) => data.extend_from_slice(sample(channel)),
                        LpcmSource::Halved(channel) => push_halved(&mut data, sample(channel)),
                        LpcmSource::Silent => data.extend(std::iter::repeat_n(0, sample_bytes)),
                    }
                }
                data.extend_from_slice(&silent);
            }
        }
        // The last unit is filled up with silence.
        data.resize(4 + unit_payload, 0);

        let pts = unit_pts(start_pts, index)?;

        units.push(EsUnit {
            pts,
            dts: pts,
            data,
            random_access: true,
        });
    }

    Ok(units)
}

/// Build the four-byte BD LPCM unit header: the payload's size (16 bits), the channel assignment (4 bits:
/// [`bd_lpcm_layout`]) and the sampling frequency (4: 1 = 48 kHz, 4 = 96 kHz, 5 = 192 kHz), then the bits a
/// sample (2: 1 = 16, 3 = 24) in the top of the last byte.
fn bd_header(
    assignment: u8,
    payload: usize,
    rate_code: u8,
    bits_code: u8,
) -> Result<[u8; 4], M2tsError> {
    let [high, low] = u16::try_from(payload)
        .map_err(|_| M2tsError("BD LPCM unit is too large".to_string()))?
        .to_be_bytes();
    Ok([
        high,
        low,
        (assignment << 4) | (rate_code & 0x0F),
        (bits_code & 0x03) << 6,
    ])
}

/// Compute the PTS for one LPCM unit.
fn unit_pts(start_pts: u64, index: usize) -> Result<u64, M2tsError> {
    let index =
        u64::try_from(index).map_err(|_| M2tsError("LPCM unit index is too large".to_string()))?;

    let offset = index
        .checked_mul(PTS_PER_UNIT)
        .ok_or_else(|| M2tsError("LPCM PTS overflow".to_string()))?;

    start_pts
        .checked_add(offset)
        .ok_or_else(|| M2tsError("LPCM PTS overflow".to_string()))
}

#[cfg(test)]
mod tests {
    use super::{BC, BL, BR, FC, FL, FR, LFE, SL, SR, bd_lpcm_layout, bd_lpcm_units};
    use crate::M2tsError;

    /// M6.366: a header written for a WAV is read back as that WAV's channels, rate and sample size.
    #[test]
    fn a_header_reads_back_as_it_was_written() -> Result<(), M2tsError> {
        use super::{LFE, bd_lpcm_layout, bd_lpcm_units_of, read_bd_lpcm_header};
        for (channels, mask, bits, rate) in [
            (1u8, 0u32, 16u8, 48_000u32),
            (2, 0, 24, 96_000),
            (6, FL | FR | FC | LFE | BL | BR, 16, 48_000),
            (8, FL | FR | FC | LFE | BL | BR | SL | SR, 24, 48_000),
            (3, FL | FR | BC, 16, 48_000),
        ] {
            let stored = usize::from(channels.max(2).next_multiple_of(2));
            let frame = stored * usize::from(bits / 8);
            let samples = vec![0u8; usize::from(channels) * usize::from(bits / 8) * 240];
            let units = bd_lpcm_units_of(&samples, channels, mask, (bits, rate), 0)?;
            let header = units
                .first()
                .map(|unit| unit.data.clone())
                .unwrap_or_default();
            let read =
                read_bd_lpcm_header(&header).ok_or_else(|| M2tsError("no header".to_string()))?;
            assert_eq!(
                (read.channels, usize::from(read.stored)),
                (channels, stored)
            );
            assert_eq!((read.bits, read.rate), (bits, rate));
            assert_eq!(
                usize::from(read.stored) * usize::from(read.sample_bytes),
                frame
            );
            // The stream's channel i is the WAV's channel the writer took it from.
            let (_, order) =
                bd_lpcm_layout(channels, mask).ok_or_else(|| M2tsError("no layout".to_string()))?;
            let wanted: Vec<super::LpcmSource> = read
                .wav_order
                .iter()
                .map(|channel| super::LpcmSource::Channel(*channel))
                .collect();
            assert_eq!(wanted, order, "{channels}");
        }
        assert_eq!(read_bd_lpcm_header(&[0, 0, 0x30, 0x40]), None);
        assert_eq!(read_bd_lpcm_header(&[0, 0, 0x21, 0x40]), None);
        Ok(())
    }

    /// M13.16: 24 bits and 96 kHz: the header says so, and a unit is still 5 ms.
    #[test]
    fn units_of_24_bits_and_96_khz() -> Result<(), M2tsError> {
        use super::bd_lpcm_units_of;
        // Stereo at 96 kHz, 24 bits: 480 frames of 6 bytes a unit; a unit and a half of sound.
        let samples: Vec<u8> = (0..720 * 6).map(|byte| (byte % 251) as u8).collect();
        let units = bd_lpcm_units_of(&samples, 2, 0, (24, 96_000), 900)?;
        assert_eq!(units.len(), 2);
        assert_eq!(&units[0].data[..4], &[0x0B, 0x40, 0x34, 0xC0]);
        assert_eq!(units[0].data.len(), 4 + 2880);
        assert_eq!(&units[0].data[4..], &samples[..2880]);
        // The second unit is filled up with silence.
        assert_eq!(units[1].data.len(), 4 + 2880);
        assert_eq!(&units[1].data[4..4 + 1440], &samples[2880..]);
        assert!(units[1].data[4 + 1440..].iter().all(|byte| *byte == 0));
        assert_eq!((units[0].pts, units[1].pts), (900, 1350));
        // Mono at 48 kHz, 24 bits: each sample with a silent one beside it.
        let units = bd_lpcm_units_of(&[1, 2, 3, 4, 5, 6], 1, 0, (24, 48_000), 0)?;
        assert_eq!(&units[0].data[..4], &[0x05, 0xA0, 0x11, 0xC0]);
        assert_eq!(&units[0].data[4..16], &[1, 2, 3, 0, 0, 0, 4, 5, 6, 0, 0, 0]);
        assert_eq!(units[0].data.len(), 4 + 1440);
        // What a Blu-ray's stream here does not hold.
        assert!(bd_lpcm_units_of(&[0; 12], 2, 0, (20, 48_000), 0).is_err());
        assert!(bd_lpcm_units_of(&[0; 12], 2, 0, (16, 44_100), 0).is_err());
        // Speakers no assignment takes, even on a larger one (front centre and LFE only).
        assert!(bd_lpcm_units_of(&[0; 4], 2, 0, (16, 48_000), 0).is_ok());
        assert!(bd_lpcm_layout(3, FC | LFE | BC).is_none());
        // Eight channels of 24 bits at 192 kHz are past 27.648 Mbit/s.
        assert!(bd_lpcm_units_of(&[0; 24], 8, 0, (24, 192_000), 0).is_err());
        Ok(())
    }

    /// M6.347: more than two channels go in the stream's order with the assignment's code, an odd count with a
    /// silent channel more; 192 kHz units are 960 sample frames.
    #[test]
    fn channels_go_in_the_streams_order() -> Result<(), M2tsError> {
        use super::{LpcmSource::Channel, bd_lpcm_layout, bd_lpcm_units_of};
        // 16-bit samples: channel n's sample is n + 1.
        let frame = |channels: u16| -> Vec<u8> {
            (1..=channels)
                .flat_map(|channel| channel.to_be_bytes())
                .collect()
        };
        let values = |data: &[u8], count: usize| -> Vec<u16> {
            data[4..4 + 2 * count]
                .chunks(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect()
        };
        // 5.1 (FL FR FC LFE BL BR): L R C Ls Rs LFE, assignment 9.
        let units = bd_lpcm_units_of(&frame(6), 6, 0, (16, 48_000), 0)?;
        assert_eq!(&units[0].data[..4], &[0x0B, 0x40, 0x91, 0x40]);
        assert_eq!(values(&units[0].data, 6), [1, 2, 3, 5, 6, 4]);
        // The same with side surrounds (named by the file).
        let units = bd_lpcm_units_of(&frame(6), 6, 0x60F, (16, 48_000), 0)?;
        assert_eq!(values(&units[0].data, 6), [1, 2, 3, 5, 6, 4]);
        // 3.0 (FL FR FC): L R C and a silent channel, assignment 4.
        let units = bd_lpcm_units_of(&frame(3), 3, 0, (16, 48_000), 0)?;
        assert_eq!(&units[0].data[..4], &[0x07, 0x80, 0x41, 0x40]);
        assert_eq!(values(&units[0].data, 4), [1, 2, 3, 0]);
        // 7.1 (FL FR FC LFE BL BR SL SR): L R C Ls(side) Rls Rrs Rs(side) LFE, assignment 11.
        let units = bd_lpcm_units_of(&frame(8), 8, 0, (16, 48_000), 0)?;
        assert_eq!(units[0].data[2], 0xB1);
        assert_eq!(values(&units[0].data, 8), [1, 2, 3, 7, 5, 6, 8, 4]);
        // The other assignments by the speakers a file names.
        let channels = |order: &[usize]| {
            order
                .iter()
                .map(|channel| Channel(*channel))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            bd_lpcm_layout(3, FL | FR | BC),
            Some((5, channels(&[0, 1, 2])))
        );
        assert_eq!(
            bd_lpcm_layout(4, FL | FR | FC | BC),
            Some((6, channels(&[0, 1, 2, 3])))
        );
        assert_eq!(bd_lpcm_layout(4, 0), Some((7, channels(&[0, 1, 2, 3]))));
        assert_eq!(bd_lpcm_layout(5, 0), Some((8, channels(&[0, 1, 2, 3, 4]))));
        assert_eq!(
            bd_lpcm_layout(7, FL | FR | FC | BL | BR | SL | SR),
            Some((10, channels(&[0, 1, 2, 5, 3, 4, 6])))
        );
        // 192 kHz stereo: 960 frames of 4 bytes, sampling frequency code 5.
        let units = bd_lpcm_units_of(&[0; 4], 2, 0, (16, 192_000), 0)?;
        assert_eq!(&units[0].data[..4], &[0x0F, 0x00, 0x35, 0x40]);
        assert_eq!(units[0].data.len(), 4 + 3840);
        Ok(())
    }

    /// M6.544: speakers no assignment has go on a larger one: 2.1 and 4.1 on 3/2 with LFE (the missing speakers
    /// silent), 6.1 on 3/4 with LFE (the back centre in both back speakers, 3 dB down), a lone surround in both
    /// surrounds 3 dB down; seven channels that name none are 6.1. The stream stores the larger layout's channels.
    #[test]
    fn layouts_without_an_assignment_go_on_a_larger_one() -> Result<(), M2tsError> {
        use super::{bd_lpcm_stored, bd_lpcm_units_of};
        // 16-bit samples: channel n's sample is 1000 × (n + 1).
        let frame = |channels: i16| -> Vec<u8> {
            (1..=channels)
                .flat_map(|channel| (channel * 1000).to_be_bytes())
                .collect()
        };
        let values = |data: &[u8], count: usize| -> Vec<i16> {
            data[4..4 + 2 * count]
                .chunks(2)
                .map(|pair| i16::from_be_bytes([pair[0], pair[1]]))
                .collect()
        };
        // 2.1 (FL FR LFE): L R, silent C Ls Rs, LFE; assignment 9.
        let units = bd_lpcm_units_of(&frame(3), 3, FL | FR | LFE, (16, 48_000), 0)?;
        assert_eq!(units[0].data[2] >> 4, 9);
        assert_eq!(values(&units[0].data, 6), [1000, 2000, 0, 0, 0, 3000]);
        assert_eq!(units[0].data.len(), 4 + 240 * 6 * 2);
        // 4.1 (FL FR LFE BL BR): L R, silent C, Ls Rs, LFE.
        let units = bd_lpcm_units_of(&frame(5), 5, FL | FR | LFE | BL | BR, (16, 48_000), 0)?;
        assert_eq!(values(&units[0].data, 6), [1000, 2000, 0, 4000, 5000, 3000]);
        // 6.1 (FL FR FC LFE BC SL SR): L R C Ls, BC in both backs 3 dB down, Rs, LFE; assignment 11. Unnamed seven
        // channels are the same.
        for mask in [FL | FR | FC | LFE | BC | SL | SR, 0] {
            let units = bd_lpcm_units_of(&frame(7), 7, mask, (16, 48_000), 0)?;
            assert_eq!(units[0].data[2] >> 4, 11);
            assert_eq!(
                values(&units[0].data, 8),
                [1000, 2000, 3000, 6000, 3536, 3536, 7000, 4000]
            );
        }
        // 2/1 with LFE (FL FR LFE BC): the surround in both, 3 dB down; a negative 24-bit sample stays negative.
        let sample = (-800_000i32).to_be_bytes();
        let mut frame24 = Vec::new();
        for value in [100i32, 200, 300] {
            frame24.extend_from_slice(&value.to_be_bytes()[1..]);
        }
        frame24.extend_from_slice(&sample[1..]);
        let units = bd_lpcm_units_of(&frame24, 4, FL | FR | LFE | BC, (24, 48_000), 0)?;
        let at = |slot: usize| {
            let bytes = &units[0].data[4 + 3 * slot..7 + 3 * slot];
            (i32::from_be_bytes([bytes[0], bytes[1], bytes[2], 0])) >> 8
        };
        assert_eq!((at(0), at(1), at(2), at(5)), (100, 200, 0, 300));
        assert_eq!((at(3), at(4)), (-565_685, -565_685));
        assert_eq!(bd_lpcm_stored(3, FL | FR | LFE), Some(6));
        assert_eq!(bd_lpcm_stored(7, 0), Some(8));
        assert_eq!(bd_lpcm_stored(3, 0), Some(4));
        Ok(())
    }

    #[test]
    fn stereo_header() -> Result<(), M2tsError> {
        let samples = vec![0_u8; 240 * 4];
        let units = bd_lpcm_units(&samples, 2, 0)?;

        assert_eq!(units.len(), 1);
        assert_eq!(&units[0].data[..4], &[0x03, 0xC0, 0x31, 0x40]);
        assert_eq!(units[0].data.len(), 4 + 960);

        Ok(())
    }

    #[test]
    fn mono_header_and_padding() -> Result<(), M2tsError> {
        let mut samples = vec![0_u8; 240 * 2];
        samples[0] = 0x12;
        samples[1] = 0x34;

        let units = bd_lpcm_units(&samples, 1, 0)?;

        assert_eq!(units.len(), 1);
        assert_eq!(&units[0].data[..4], &[0x03, 0xC0, 0x11, 0x40]);
        assert_eq!(&units[0].data[4..8], &[0x12, 0x34, 0x00, 0x00]);

        Ok(())
    }

    #[test]
    fn last_unit_is_padded() -> Result<(), M2tsError> {
        let samples = vec![1_u8; 1000 * 4];
        let units = bd_lpcm_units(&samples, 2, 0)?;

        assert_eq!(units.len(), 5);
        assert_eq!(units[4].data.len(), 4 + 960);
        assert!(units[4].data[4 + 160..].iter().all(|&byte| byte == 0));

        Ok(())
    }
}
