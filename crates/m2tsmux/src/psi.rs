//! Program Association Table and Program Map Table sections.

use crate::M2tsError;

/// One elementary stream entry for the PMT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PmtStream {
    /// PMT `stream_type`.
    pub(crate) stream_type: u8,
    /// Elementary stream PID.
    pub(crate) pid: u16,
}

/// Compute the MPEG-2 CRC-32 used by PSI sections.
pub(crate) fn crc32_mpeg(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;

    for &byte in data {
        crc ^= u32::from(byte) << 24;

        for _ in 0..8 {
            if crc & 0x8000_0000 != 0 {
                crc = (crc << 1) ^ 0x04C1_1DB7;
            } else {
                crc <<= 1;
            }
        }
    }

    crc
}

/// Build one PAT section.
pub(crate) fn pat_section(program_number: u16, pmt_pid: u16) -> Vec<u8> {
    let mut body = Vec::new();

    body.extend_from_slice(&0_u16.to_be_bytes());
    body.push(0xC1);
    body.push(0x00);
    body.push(0x00);

    body.extend_from_slice(&program_number.to_be_bytes());
    push_pid_field(&mut body, pmt_pid);

    finish_section(0x00, &body)
}

/// Build one PMT section.
pub(crate) fn pmt_section(
    program_number: u16,
    pcr_pid: u16,
    streams: &[PmtStream],
) -> Result<Vec<u8>, M2tsError> {
    let mut body = Vec::new();

    body.extend_from_slice(&program_number.to_be_bytes());
    body.push(0xC1);
    body.push(0x00);
    body.push(0x00);

    push_pid_field(&mut body, pcr_pid);
    // program_info_length counts the whole descriptor: tag, length and the four identifier bytes.
    push_length_field(&mut body, 6);

    body.extend_from_slice(&[0x05, 0x04, b'H', b'D', b'M', b'V']);

    for stream in streams {
        body.push(stream.stream_type);
        push_pid_field(&mut body, stream.pid);
        push_length_field(&mut body, 0);
    }

    Ok(finish_section(0x02, &body))
}

/// Append a `reserved(3) + PID(13)` field.
fn push_pid_field(out: &mut Vec<u8>, pid: u16) {
    let high = 0xE0 | u8::try_from((pid >> 8) & 0x1F).unwrap_or(0);
    let low = u8::try_from(pid & 0xFF).unwrap_or(0);

    out.push(high);
    out.push(low);
}

/// Append a `reserved(4) + length(12)` field.
fn push_length_field(out: &mut Vec<u8>, length: usize) {
    let high = 0xF0 | u8::try_from((length >> 8) & 0x0F).unwrap_or(0);
    let low = u8::try_from(length & 0xFF).unwrap_or(0);

    out.push(high);
    out.push(low);
}

/// Add the PSI header and CRC to a section body.
fn finish_section(table_id: u8, body: &[u8]) -> Vec<u8> {
    let section_length = body.len() + 4;
    let high = 0xB0 | u8::try_from((section_length >> 8) & 0x0F).unwrap_or(0);
    let low = u8::try_from(section_length & 0xFF).unwrap_or(0);

    let mut out = Vec::with_capacity(3 + section_length);
    out.push(table_id);
    out.push(high);
    out.push(low);
    out.extend_from_slice(body);

    let crc = crc32_mpeg(&out);
    out.extend_from_slice(&crc.to_be_bytes());

    out
}

#[cfg(test)]
mod tests {
    use super::{PmtStream, crc32_mpeg, pat_section, pmt_section};
    use crate::{M2tsError, PMT_PID};

    #[test]
    fn pat_crc_matches_known_section() {
        let pat = pat_section(1, PMT_PID);

        assert!(pat.len() >= 4);
        let without_crc = &pat[..pat.len() - 4];
        let crc = crc32_mpeg(without_crc);

        assert_eq!(&pat[pat.len() - 4..], &crc.to_be_bytes());
        assert_eq!(crc32_mpeg(&pat), 0);
        assert_eq!(
            &pat[..12],
            &[
                0x00, 0xB0, 0x0D, 0x00, 0x00, 0xC1, 0x00, 0x00, 0x00, 0x01, 0xE1, 0x00
            ]
        );
    }

    #[test]
    fn pmt_contains_registration_descriptor() -> Result<(), M2tsError> {
        let streams = [PmtStream {
            stream_type: 0x02,
            pid: 0x1011,
        }];

        let pmt = pmt_section(1, 0x1001, &streams)?;

        assert_eq!(pmt[0], 0x02);
        assert!(
            pmt.windows(8)
                .any(|w| w == [0xF0, 0x06, 0x05, 0x04, b'H', b'D', b'M', b'V'])
        );
        assert_eq!(crc32_mpeg(&pmt), 0);

        Ok(())
    }
}
