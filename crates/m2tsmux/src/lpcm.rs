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

/// The speakers of a WAV's `channels` when the file names none: the usual ones of that count (none for seven,
/// whose usual layouts differ).
fn usual_speakers(channels: u8) -> Option<u32> {
    match channels {
        3 => Some(FL | FR | FC),
        4 => Some(FL | FR | BL | BR),
        5 => Some(FL | FR | FC | BL | BR),
        6 => Some(FL | FR | FC | LFE | BL | BR),
        8 => Some(FL | FR | FC | LFE | BL | BR | SL | SR),
        _ => None,
    }
}

/// The BD LPCM channel assignment of a WAV's `channels` on speakers `mask` (its `dwChannelMask`; 0, or a mask of
/// another count, for the usual ones), and for each channel of the stream, in the stream's order, the WAV channel
/// it is. One channel is mono and two stereo whatever their speakers. `None` for speakers no assignment has (2.1,
/// 4.1, 6.1, seven channels that name none).
#[must_use]
pub fn bd_lpcm_layout(channels: u8, mask: u32) -> Option<(u8, Vec<usize>)> {
    match channels {
        1 => return Some((1, vec![0])),
        2 => return Some((3, vec![0, 1])),
        _ => {}
    }
    let mask = if mask.count_ones() == u32::from(channels) {
        mask
    } else {
        usual_speakers(channels)?
    };
    let (code, speakers) = LAYOUTS
        .iter()
        .find(|(_, speakers)| speakers.iter().fold(0, |all, speaker| all | speaker) == mask)?;
    // A WAV's channels are in the order of their speakers' bits.
    let order = speakers
        .iter()
        .map(|speaker| usize::try_from((mask & speaker.wrapping_sub(1)).count_ones()).ok())
        .collect::<Option<Vec<_>>>()?;
    Some((*code, order))
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
    let stored = usize::from(channels.max(2).next_multiple_of(2));
    let stored_bits = u64::from(sample_rate) * u64::from(bits) * stored as u64;
    if stored_bits > MAX_BITRATE {
        return Err(M2tsError("BD LPCM is at most 27.648 Mbit/s".to_string()));
    }
    // What a unit holds: its frames of the stored channels.
    let unit_payload = frames_per_unit * stored * sample_bytes;

    let frame_bytes = usize::from(channels) * sample_bytes;

    if !samples.len().is_multiple_of(frame_bytes) {
        return Err(M2tsError(
            "BD LPCM sample byte count is not a whole frame".to_string(),
        ));
    }

    let total_frames = samples.len() / frame_bytes;
    let unit_count = total_frames.div_ceil(frames_per_unit);
    let header = bd_header(assignment, unit_payload, rate_code, bits_code)?;
    // Channels in the WAV's order need no reordering.
    let in_order = order
        .iter()
        .enumerate()
        .all(|(slot, channel)| slot == *channel);
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
                for channel in &order {
                    let at = channel * sample_bytes;
                    data.extend_from_slice(frame.get(at..at + sample_bytes).unwrap_or_default());
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
    use super::{BC, BL, BR, FC, FL, FR, SL, SR, bd_lpcm_units};
    use crate::M2tsError;

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
        // Seven channels that name no speakers, and 2.1, have no assignment.
        assert!(bd_lpcm_units_of(&[0; 14], 7, 0, (16, 48_000), 0).is_err());
        assert!(bd_lpcm_units_of(&[0; 6], 3, 0b1011, (16, 48_000), 0).is_err());
        // Eight channels of 24 bits at 192 kHz are past 27.648 Mbit/s.
        assert!(bd_lpcm_units_of(&[0; 24], 8, 0, (24, 192_000), 0).is_err());
        Ok(())
    }

    /// M6.347: more than two channels go in the stream's order with the assignment's code, an odd count with a
    /// silent channel more; 192 kHz units are 960 sample frames.
    #[test]
    fn channels_go_in_the_streams_order() -> Result<(), M2tsError> {
        use super::{bd_lpcm_layout, bd_lpcm_units_of};
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
        assert_eq!(bd_lpcm_layout(3, FL | FR | BC), Some((5, vec![0, 1, 2])));
        assert_eq!(
            bd_lpcm_layout(4, FL | FR | FC | BC),
            Some((6, vec![0, 1, 2, 3]))
        );
        assert_eq!(bd_lpcm_layout(4, 0), Some((7, vec![0, 1, 2, 3])));
        assert_eq!(bd_lpcm_layout(5, 0), Some((8, vec![0, 1, 2, 3, 4])));
        assert_eq!(
            bd_lpcm_layout(7, FL | FR | FC | BL | BR | SL | SR),
            Some((10, vec![0, 1, 2, 5, 3, 4, 6]))
        );
        // 192 kHz stereo: 960 frames of 4 bytes, sampling frequency code 5.
        let units = bd_lpcm_units_of(&[0; 4], 2, 0, (16, 192_000), 0)?;
        assert_eq!(&units[0].data[..4], &[0x0F, 0x00, 0x35, 0x40]);
        assert_eq!(units[0].data.len(), 4 + 3840);
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
