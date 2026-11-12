//! BD LPCM access-unit framing.

use crate::{EsUnit, M2tsError};

const FRAMES_PER_UNIT: usize = 240;
const BYTES_PER_SAMPLE: usize = 2;
const UNIT_PAYLOAD_BYTES: usize = 960;
const PTS_PER_UNIT: u64 = 450;

/// Split big-endian 16-bit 48 kHz samples into 5 ms BD LPCM units.
pub fn bd_lpcm_units(
    samples: &[u8],
    channels: u8,
    start_pts: u64,
) -> Result<Vec<EsUnit>, M2tsError> {
    if channels != 1 && channels != 2 {
        return Err(M2tsError(
            "BD LPCM requires one or two channels".to_string(),
        ));
    }

    let frame_bytes = usize::from(channels) * BYTES_PER_SAMPLE;

    if !samples.len().is_multiple_of(frame_bytes) {
        return Err(M2tsError(
            "BD LPCM sample byte count is not a whole frame".to_string(),
        ));
    }

    let total_frames = samples.len() / frame_bytes;
    let unit_count = total_frames.div_ceil(FRAMES_PER_UNIT);
    let header = bd_header(channels);

    let mut units = Vec::with_capacity(unit_count);

    for index in 0..unit_count {
        let start_frame = index * FRAMES_PER_UNIT;
        let frames = (total_frames - start_frame).min(FRAMES_PER_UNIT);
        let offset = start_frame * frame_bytes;
        let len = frames * frame_bytes;

        let chunk: Vec<u8> = samples.iter().skip(offset).take(len).copied().collect();

        let data = if channels == 1 {
            mono_unit_data(&chunk, &header)
        } else {
            stereo_unit_data(&chunk, &header)
        };

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

/// Build the four-byte BD LPCM unit header.
fn bd_header(channels: u8) -> [u8; 4] {
    let channel_assignment = if channels == 1 { 1 } else { 3 };
    let byte2 = u8::try_from((channel_assignment << 4) | 1).unwrap_or(0);

    [0x03, 0xC0, byte2, 0x40]
}

/// Convert mono frames to padded stereo unit data.
fn mono_unit_data(frames: &[u8], header: &[u8; 4]) -> Vec<u8> {
    let mut data = Vec::with_capacity(4 + UNIT_PAYLOAD_BYTES);
    data.extend_from_slice(header);

    let mut iter = frames.iter();

    for _ in 0..FRAMES_PER_UNIT {
        if let Some(&byte) = iter.next() {
            data.push(byte);
        } else {
            data.push(0);
        }

        if let Some(&byte) = iter.next() {
            data.push(byte);
        } else {
            data.push(0);
        }

        data.push(0);
        data.push(0);
    }

    data
}

/// Copy stereo frames and pad the last unit with zero samples.
fn stereo_unit_data(frames: &[u8], header: &[u8; 4]) -> Vec<u8> {
    let mut data = Vec::with_capacity(4 + UNIT_PAYLOAD_BYTES);
    data.extend_from_slice(header);
    data.extend_from_slice(frames);
    data.resize(4 + UNIT_PAYLOAD_BYTES, 0);

    data
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
    use super::bd_lpcm_units;
    use crate::M2tsError;

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
