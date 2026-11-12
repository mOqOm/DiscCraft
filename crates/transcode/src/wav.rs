use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::{TranscodeError, io_error};

const HEADER_BYTES: u64 = 44;
const MAX_FILE_BYTES: u64 = 4_294_967_296;
pub(crate) const MAX_DATA_BYTES: u64 = MAX_FILE_BYTES - HEADER_BYTES;

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const BITS_PER_SAMPLE: u16 = 16;
const BYTE_RATE: u32 = SAMPLE_RATE * CHANNELS as u32 * (BITS_PER_SAMPLE as u32 / 8);
const BLOCK_ALIGN: u16 = CHANNELS * (BITS_PER_SAMPLE / 8);

/// Streaming PCM WAV writer with fixed sizes on finish.
pub(crate) struct WavWriter {
    path: PathBuf,
    writer: BufWriter<File>,
    data_bytes: u64,
}

impl WavWriter {
    /// Create a WAV file and write a placeholder header.
    pub(crate) fn new(path: &Path) -> Result<Self, TranscodeError> {
        let file = File::create(path).map_err(|error| io_error(path, error))?;
        let mut writer = BufWriter::new(file);
        write_header(&mut writer, path)?;

        Ok(Self {
            path: path.to_path_buf(),
            writer,
            data_bytes: 0,
        })
    }

    /// Append interleaved 16-bit little-endian samples.
    pub(crate) fn write_samples(&mut self, samples: &[i16]) -> Result<(), TranscodeError> {
        let bytes = u64::try_from(samples.len())
            .ok()
            .and_then(|count| count.checked_mul(2))
            .ok_or_else(|| TranscodeError::Unsupported("sample count too large".to_string()))?;

        let next = self
            .data_bytes
            .checked_add(bytes)
            .ok_or_else(|| TranscodeError::Unsupported("WAV size overflow".to_string()))?;

        if next > MAX_DATA_BYTES {
            return Err(TranscodeError::Unsupported(
                "WAV output exceeds 4 GiB".to_string(),
            ));
        }

        for chunk in samples.chunks(4096) {
            let capacity = chunk
                .len()
                .checked_mul(2)
                .ok_or_else(|| TranscodeError::Unsupported("WAV chunk overflow".to_string()))?;
            let mut buffer = Vec::with_capacity(capacity);

            for sample in chunk {
                buffer.extend_from_slice(&sample.to_le_bytes());
            }

            self.writer
                .write_all(&buffer)
                .map_err(|error| io_error(&self.path, error))?;
        }

        self.data_bytes = next;
        Ok(())
    }

    /// Flush data and patch RIFF and data chunk sizes.
    pub(crate) fn finish(self) -> Result<(), TranscodeError> {
        let mut writer = self.writer;
        writer
            .flush()
            .map_err(|error| io_error(&self.path, error))?;

        if self.data_bytes > MAX_DATA_BYTES {
            return Err(TranscodeError::Unsupported(
                "WAV output exceeds 4 GiB".to_string(),
            ));
        }

        let riff_size = self
            .data_bytes
            .checked_add(36)
            .ok_or_else(|| TranscodeError::Unsupported("WAV RIFF size overflow".to_string()))?;

        let riff_size = u32::try_from(riff_size)
            .map_err(|_| TranscodeError::Unsupported("WAV RIFF size too large".to_string()))?;
        let data_size = u32::try_from(self.data_bytes)
            .map_err(|_| TranscodeError::Unsupported("WAV data size too large".to_string()))?;

        writer
            .seek(SeekFrom::Start(4))
            .map_err(|error| io_error(&self.path, error))?;
        write_u32(&mut writer, riff_size, &self.path)?;

        writer
            .seek(SeekFrom::Start(40))
            .map_err(|error| io_error(&self.path, error))?;
        write_u32(&mut writer, data_size, &self.path)?;

        writer
            .flush()
            .map_err(|error| io_error(&self.path, error))?;
        Ok(())
    }
}

fn write_header(writer: &mut BufWriter<File>, path: &Path) -> Result<(), TranscodeError> {
    writer
        .write_all(b"RIFF")
        .map_err(|error| io_error(path, error))?;
    write_u32(writer, 0, path)?;
    writer
        .write_all(b"WAVE")
        .map_err(|error| io_error(path, error))?;

    writer
        .write_all(b"fmt ")
        .map_err(|error| io_error(path, error))?;
    write_u32(writer, 16, path)?;
    write_u16(writer, 1, path)?;
    write_u16(writer, CHANNELS, path)?;
    write_u32(writer, SAMPLE_RATE, path)?;
    write_u32(writer, BYTE_RATE, path)?;
    write_u16(writer, BLOCK_ALIGN, path)?;
    write_u16(writer, BITS_PER_SAMPLE, path)?;

    writer
        .write_all(b"data")
        .map_err(|error| io_error(path, error))?;
    write_u32(writer, 0, path)?;
    Ok(())
}

fn write_u32<W: Write>(writer: &mut W, value: u32, path: &Path) -> Result<(), TranscodeError> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|error| io_error(path, error))
}

fn write_u16<W: Write>(writer: &mut W, value: u16, path: &Path) -> Result<(), TranscodeError> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|error| io_error(path, error))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::WavWriter;
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::process;

    #[test]
    fn writes_header_and_sizes() -> Result<(), String> {
        let path = temp_path("wav");
        let _ = fs::remove_file(&path);

        {
            let mut writer = WavWriter::new(&path).map_err(|error| error.to_string())?;
            writer
                .write_samples(&[0, 0, 0, 0])
                .map_err(|error| error.to_string())?;
            writer.finish().map_err(|error| error.to_string())?;
        }

        let data = fs::read(&path).map_err(|error| error.to_string())?;
        assert_eq!(data.len(), 52);
        assert_eq!(&data[0..4], b"RIFF");
        assert_eq!(&data[4..8], &44u32.to_le_bytes());
        assert_eq!(&data[8..12], b"WAVE");
        assert_eq!(&data[12..16], b"fmt ");
        assert_eq!(&data[20..22], &1u16.to_le_bytes());
        assert_eq!(&data[22..24], &2u16.to_le_bytes());
        assert_eq!(&data[24..28], &48_000u32.to_le_bytes());
        assert_eq!(&data[36..40], b"data");
        assert_eq!(&data[40..44], &8u32.to_le_bytes());

        let _ = fs::remove_file(&path);
        Ok(())
    }

    fn temp_path(name: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "disccraft-transcode-{}-{}.wav",
            process::id(),
            name
        ))
    }
}
