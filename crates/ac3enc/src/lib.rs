//! AC-3 エンコーダ（モノラル / ステレオ）。
//!
//! 層: L0（docs/architecture.md §5）。
//!
//! DVD / Blu-ray の変換した音声（48 kHz・16 ビットの WAV）を AC-3（Dolby Digital）の生のストリームにする。符号化そのものは
//! craft-libs の `craft-audio`（MIT OR Apache-2.0、ETSI TS 102 366 から書かれた AC-3 エンコーダ）を公開 API から使う
//! （その実装は読まない。AGENTS.md ルール 3）。このクレートは WAV の読み取りと、エンコーダの前置きの分だけ映像とずれない
//! ようにする並べ方を受け持つ。
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use craft_audio::ac3::encoder::{Codec, CodingMode, Config, Encoder};

/// The rate of a stereo soundtrack: 192 kbit/s, as Encore transcodes a stereo source for DVD.
pub const STEREO_KBPS: u32 = 192;
/// The rate of a mono soundtrack.
pub const MONO_KBPS: u32 = 96;

/// The sample rate DVD and Blu-ray audio uses.
const SAMPLE_RATE: u32 = 48_000;
/// Samples per channel of one AC-3 frame.
const FRAME_SAMPLES: usize = 1536;
/// Frames of PCM read at a time.
const CHUNK_FRAMES: usize = FRAME_SAMPLES * 16;
/// The longest RIFF chunk header search before the data (a WAV with more is not one of ours).
const MAX_CHUNKS: usize = 64;

/// Errors from reading the WAV or encoding.
#[derive(Debug, thiserror::Error)]
pub enum Ac3Error {
    /// Reading or writing a file failed.
    #[error("{path}: {message}")]
    Io {
        /// The file.
        path: String,
        /// What went wrong.
        message: String,
    },
    /// The WAV is not 48 kHz, 16-bit PCM in one or two channels.
    #[error("unsupported WAV: {0}")]
    Unsupported(String),
    /// The encoder refused the input.
    #[error("AC-3 encoder: {0}")]
    Encoder(String),
}

/// What an encode wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Encoded {
    /// AC-3 frames written (1536 samples each).
    pub frames: u64,
    /// Channels of the source (1 or 2).
    pub channels: u16,
    /// The rate in kbit/s.
    pub kbps: u32,
}

/// The rate for `channels` of audio.
pub fn kbps_for(channels: u16) -> u32 {
    if channels == 1 {
        MONO_KBPS
    } else {
        STEREO_KBPS
    }
}

/// Encodes the 48 kHz 16-bit PCM WAV `wav` (one or two channels) into the raw AC-3 stream `out`, at
/// [`kbps_for`] its channels.
///
/// The encoder's analysis window delays the decoded sound by its priming samples (256); as many samples are taken off
/// the front of the PCM, so the decoded stream lines up with the source sample for sample (its first 5 ms are the
/// encoder's silence). The stream ends with the last frame that holds source samples.
///
/// # Errors
///
/// [`Ac3Error::Io`] when a file cannot be read or written, [`Ac3Error::Unsupported`] for a WAV of another kind and
/// [`Ac3Error::Encoder`] when the encoder refuses it.
pub fn encode_wav(wav: &Path, out: &Path) -> Result<Encoded, Ac3Error> {
    let io = |path: &Path, error: std::io::Error| Ac3Error::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    };
    let file = File::open(wav).map_err(|e| io(wav, e))?;
    let size = file.metadata().map_err(|e| io(wav, e))?.len();
    let mut reader = BufReader::new(file);
    let format = read_header(&mut reader, size).map_err(|e| match e {
        HeaderError::Io(error) => io(wav, error),
        HeaderError::Bad(message) => Ac3Error::Unsupported(message),
    })?;

    let data_start = reader.stream_position().map_err(|e| io(wav, e))?;
    drop(reader);
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    encode_data(wav, out, &format, data_start, SEGMENT_FRAMES, threads)
}

/// [`encode_wav`] of the PCM at `data_start`, in segments of `segment_frames` AC-3 frames on up to `threads`
/// threads.
///
/// AC-3 frames are coded one by one; all a frame takes from the sound before it is the tail of the transform's
/// window. So a long sound is cut into segments that are encoded side by side, each by an encoder of its own that
/// is first run over the [`LEAD_FRAMES`] frames in front of its segment (their packets are dropped). The stream is
/// the same, byte for byte, as one encoder makes from the whole sound.
fn encode_data(
    wav: &Path,
    out: &Path,
    format: &Format,
    data_start: u64,
    segment_frames: u64,
    threads: usize,
) -> Result<Encoded, Ac3Error> {
    let io = |path: &Path, error: std::io::Error| Ac3Error::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    };
    let channels = usize::from(format.channels);
    let kbps = kbps_for(format.channels);
    let frame_bytes = (channels * 2) as u64;
    // The priming samples come off the front of the source.
    let priming = u64::from(new_encoder(format.channels)?.priming_samples());
    let source = Source {
        wav,
        // Where the sound that is encoded starts, and its length in samples.
        start: data_start.saturating_add(priming.saturating_mul(frame_bytes)),
        samples: (format.data_bytes / frame_bytes).saturating_sub(priming),
        channels: format.channels,
    };
    let segment_frames = segment_frames.max(LEAD_FRAMES + 1);
    // Every segment but the last is `segment_frames` whole frames; the last takes the rest (at least as much).
    let whole_frames = source.samples / FRAME_SAMPLES as u64;
    let segments = (whole_frames / segment_frames).max(1);
    let threads = u64::try_from(threads.max(1)).unwrap_or(1).min(segments);

    let mut writer = BufWriter::new(File::create(out).map_err(|e| io(out, e))?);
    let mut frames = 0u64;
    let mut next = 0u64;
    while next < segments {
        // A batch of segments, one a thread; written in order.
        let batch: Vec<u64> = (next..segments.min(next.saturating_add(threads))).collect();
        next = next.saturating_add(batch.len() as u64);
        let encoded: Vec<Result<Vec<u8>, Ac3Error>> = if let [only] = batch.as_slice() {
            vec![source.segment(*only, segments, segment_frames)]
        } else {
            std::thread::scope(|scope| {
                let workers: Vec<_> = batch
                    .iter()
                    .map(|&segment| {
                        let source = &source;
                        scope.spawn(move || source.segment(segment, segments, segment_frames))
                    })
                    .collect();
                workers
                    .into_iter()
                    .map(|worker| {
                        worker.join().unwrap_or_else(|_| {
                            Err(Ac3Error::Encoder("the encoder thread stopped".to_string()))
                        })
                    })
                    .collect()
            })
        };
        for data in encoded {
            let data = data?;
            frames = frames.saturating_add((data.len() / frame_len(kbps).max(1)) as u64);
            writer.write_all(&data).map_err(|e| io(out, e))?;
        }
    }
    writer.flush().map_err(|e| io(out, e))?;

    Ok(Encoded {
        frames,
        channels: format.channels,
        kbps,
    })
}

/// AC-3 frames in a segment that is encoded on its own thread: a minute of sound.
const SEGMENT_FRAMES: u64 = 1875;
/// Frames an encoder is run over in front of its segment, so its first kept frame has the sound before it.
const LEAD_FRAMES: u64 = 2;

/// Bytes of one frame (1536 samples) at `kbps` and 48 kHz: 192 kbit/s is 768 bytes.
fn frame_len(kbps: u32) -> usize {
    kbps as usize * 4
}

fn new_encoder(channels: u16) -> Result<Encoder, Ac3Error> {
    Encoder::new(Config {
        codec: Codec::Ac3,
        sample_rate: SAMPLE_RATE,
        bitrate_kbps: kbps_for(channels),
        mode: if channels == 1 {
            CodingMode::Mono
        } else {
            CodingMode::Stereo
        },
        lfe: false,
    })
    .map_err(|e| Ac3Error::Encoder(e.to_string()))
}

/// The PCM that is encoded.
struct Source<'a> {
    wav: &'a Path,
    /// Byte offset of its first sample.
    start: u64,
    /// Its length, samples a channel.
    samples: u64,
    channels: u16,
}

impl Source<'_> {
    /// The AC-3 frames of segment `segment` of `segments`.
    fn segment(
        &self,
        segment: u64,
        segments: u64,
        segment_frames: u64,
    ) -> Result<Vec<u8>, Ac3Error> {
        let io = |error: std::io::Error| Ac3Error::Io {
            path: self.wav.display().to_string(),
            message: error.to_string(),
        };
        let frame_samples = FRAME_SAMPLES as u64;
        let last = segment.saturating_add(1) >= segments;
        let lead = if segment == 0 { 0 } else { LEAD_FRAMES };
        let first_frame = segment.saturating_mul(segment_frames).saturating_sub(lead);
        let first_sample = first_frame.saturating_mul(frame_samples);
        // The last segment runs to the end of the sound; the others are fed whole frames.
        let mut remaining = if last {
            self.samples.saturating_sub(first_sample)
        } else {
            lead.saturating_add(segment_frames)
                .saturating_mul(frame_samples)
        };
        let channels = usize::from(self.channels);
        let frame_bytes = channels * 2;
        let mut reader = BufReader::new(File::open(self.wav).map_err(io)?);
        reader
            .seek(SeekFrom::Start(self.start.saturating_add(
                first_sample.saturating_mul(frame_bytes as u64),
            )))
            .map_err(io)?;

        let mut encoder = new_encoder(self.channels)?;
        let mut packets = Vec::new();
        let mut bytes = vec![0u8; CHUNK_FRAMES * frame_bytes];
        let mut planes = vec![Vec::with_capacity(CHUNK_FRAMES); channels];
        while remaining > 0 {
            let want = usize::try_from(remaining.min(CHUNK_FRAMES as u64)).unwrap_or(CHUNK_FRAMES)
                * frame_bytes;
            let Some(buffer) = bytes.get_mut(..want) else {
                break;
            };
            let got = read_up_to(&mut reader, buffer).map_err(io)?;
            // Whole sample frames only (a torn last frame is dropped).
            let got = got - got % frame_bytes;
            if got == 0 {
                break; // The file is shorter than its header says.
            }
            remaining = remaining.saturating_sub((got / frame_bytes) as u64);
            for plane in &mut planes {
                plane.clear();
            }
            for frame in bytes
                .get(..got)
                .unwrap_or_default()
                .chunks_exact(frame_bytes)
            {
                let (samples, _) = frame.as_chunks::<2>();
                for (plane, sample) in planes.iter_mut().zip(samples) {
                    plane.push(f32::from(i16::from_le_bytes(*sample)) / 32768.0);
                }
            }
            let refs: Vec<&[f32]> = planes.iter().map(Vec::as_slice).collect();
            packets.extend(
                encoder
                    .push(&refs)
                    .map_err(|e| Ac3Error::Encoder(e.to_string()))?,
            );
        }
        if last {
            let finish = encoder
                .finish()
                .map_err(|e| Ac3Error::Encoder(e.to_string()))?;
            packets.extend(finish.packets);
        }
        // Without the lead, and (but for the last segment) no more than the segment's own frames.
        let lead = usize::try_from(lead).unwrap_or(usize::MAX);
        let keep = if last {
            usize::MAX
        } else {
            usize::try_from(segment_frames).unwrap_or(usize::MAX)
        };
        if !last && packets.len() < lead.saturating_add(keep) {
            return Err(Ac3Error::Encoder(
                "the encoder made fewer frames than it was given".to_string(),
            ));
        }
        let mut data = Vec::new();
        for packet in packets.into_iter().skip(lead).take(keep) {
            data.extend_from_slice(&packet.data);
        }
        Ok(data)
    }
}

/// Reads until `buf` is full or the file ends; returns the bytes read.
fn read_up_to(reader: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match reader.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

/// The PCM of a WAV: its channels and the bytes of its data chunk (bounded by the file).
#[derive(Debug, PartialEq, Eq)]
struct Format {
    channels: u16,
    data_bytes: u64,
}

#[derive(Debug)]
enum HeaderError {
    Io(std::io::Error),
    Bad(String),
}

impl From<std::io::Error> for HeaderError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Reads a RIFF WAVE header up to the start of its data chunk, which the reader is left at. `size` is the file's
/// size: a data chunk that says it is longer (or a streaming writer's `0xFFFFFFFF`) ends with the file.
fn read_header(reader: &mut (impl Read + Seek), size: u64) -> Result<Format, HeaderError> {
    let bad = |message: &str| HeaderError::Bad(message.to_string());
    let mut riff = [0u8; 12];
    reader.read_exact(&mut riff)?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return Err(bad("not a RIFF WAVE file"));
    }
    let mut channels = None;
    let mut position = 12u64;
    for _ in 0..MAX_CHUNKS {
        let mut header = [0u8; 8];
        reader.read_exact(&mut header)?;
        position = position.saturating_add(8);
        let length = u64::from(u32::from_le_bytes([
            header[4], header[5], header[6], header[7],
        ]));
        match &header[0..4] {
            b"fmt " => {
                if !(16..=1024).contains(&length) {
                    return Err(bad("bad fmt chunk"));
                }
                let mut fmt = vec![0u8; usize::try_from(length).unwrap_or(16)];
                reader.read_exact(&mut fmt)?;
                let tag = u16::from_le_bytes([fmt[0], fmt[1]]);
                let count = u16::from_le_bytes([fmt[2], fmt[3]]);
                let rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
                let bits = u16::from_le_bytes([fmt[14], fmt[15]]);
                // PCM, or WAVE_FORMAT_EXTENSIBLE (whose sub-format is PCM for 16-bit integer samples).
                if tag != 1 && tag != 0xFFFE {
                    return Err(bad("not PCM"));
                }
                if rate != SAMPLE_RATE || bits != 16 || !(1..=2).contains(&count) {
                    return Err(HeaderError::Bad(format!(
                        "{count} channels of {bits}-bit PCM at {rate} Hz (48 kHz 16-bit mono or stereo needed)"
                    )));
                }
                channels = Some(count);
                position = position.saturating_add(length);
                if length % 2 == 1 {
                    reader.seek(SeekFrom::Current(1))?;
                    position = position.saturating_add(1);
                }
            }
            b"data" => {
                let channels = channels.ok_or_else(|| bad("data before fmt"))?;
                let data_bytes = length.min(size.saturating_sub(position));
                return Ok(Format {
                    channels,
                    data_bytes,
                });
            }
            _ => {
                let skip = length.saturating_add(length % 2);
                reader.seek(SeekFrom::Current(i64::try_from(skip).unwrap_or(i64::MAX)))?;
                position = position.saturating_add(skip);
            }
        }
    }
    Err(bad("no data chunk"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use std::io::Cursor;

    fn wav(channels: u16, samples: &[i16]) -> Vec<u8> {
        let data_len = u32::try_from(samples.len() * 2).unwrap();
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&48_000u32.to_le_bytes());
        out.extend_from_slice(&(48_000 * 2 * u32::from(channels)).to_le_bytes());
        out.extend_from_slice(&(2 * channels).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }

    #[test]
    fn headers_are_read_and_bounded_by_the_file() {
        let bytes = wav(2, &[1, 2, 3, 4]);
        let size = bytes.len() as u64;
        let format = read_header(&mut Cursor::new(&bytes), size).unwrap();
        assert_eq!(
            format,
            Format {
                channels: 2,
                data_bytes: 8
            }
        );
        // A streaming writer's endless data chunk ends with the file.
        let mut endless = bytes.clone();
        endless[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        let format = read_header(&mut Cursor::new(&endless), size).unwrap();
        assert_eq!(format.data_bytes, 8);
    }

    #[test]
    fn other_wavs_are_refused_not_crashed_on() {
        // 44.1 kHz, 8-bit, 6 channels, not RIFF, truncated, data before fmt.
        let mut rate = wav(2, &[0; 4]);
        rate[24..28].copy_from_slice(&44_100u32.to_le_bytes());
        let mut six = wav(2, &[0; 4]);
        six[22..24].copy_from_slice(&6u16.to_le_bytes());
        let mut riff = wav(2, &[0; 4]);
        riff[0] = b'X';
        let truncated = wav(2, &[0; 4])[..30].to_vec();
        let mut swapped = Vec::new();
        swapped.extend_from_slice(b"RIFF\0\0\0\0WAVEdata\0\0\0\0");
        for bytes in [rate, six, riff, truncated, swapped] {
            let size = bytes.len() as u64;
            assert!(read_header(&mut Cursor::new(&bytes), size).is_err());
        }
    }

    /// A long sound is encoded in segments on several threads: the stream is the one a single encoder makes.
    #[test]
    fn segments_encoded_side_by_side_make_the_same_stream() {
        let dir = std::env::temp_dir().join(format!("disccraft-ac3enc-par-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Six seconds of a sweep with noise and a click every second, different in the two channels.
        let mut seed = 0x1234_5678_u32;
        let samples: Vec<i16> = (0..48_000 * 6 + 700)
            .flat_map(|n| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = f64::from((seed >> 16) as i16) / 32768.0;
                let t = f64::from(n) / 48_000.0;
                let sweep = (2.0 * std::f64::consts::PI * (200.0 + 300.0 * t) * t).sin();
                let click = if n % 48_000 == 100 { 0.9 } else { 0.0 };
                let left = (sweep * 0.5 + noise * 0.1 + click) * 20_000.0;
                let right = (sweep * 0.2 - noise * 0.3) * 20_000.0;
                [left as i16, right as i16]
            })
            .collect();
        let input = dir.join("long.wav");
        let bytes = wav(2, &samples);
        std::fs::write(&input, &bytes).unwrap();
        let format = read_header(&mut Cursor::new(&bytes), bytes.len() as u64).unwrap();
        let one = dir.join("one.ac3");
        let many = dir.join("many.ac3");
        let whole = encode_data(&input, &one, &format, 44, u64::MAX, 1).unwrap();
        // Segments of 7 frames, 3 at a time (and a batch of one at the end).
        let split = encode_data(&input, &many, &format, 44, 7, 3).unwrap();
        let (one, many) = (std::fs::read(&one).unwrap(), std::fs::read(&many).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(whole, split);
        assert_eq!(whole.frames, 188);
        assert_eq!(one.len(), 188 * 768);
        let different: Vec<usize> = one
            .chunks(768)
            .zip(many.chunks(768))
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(frame, _)| frame)
            .collect();
        assert!(
            different.is_empty() && one.len() == many.len(),
            "{different:?}"
        );
    }

    #[test]
    fn a_tone_becomes_ac3_frames_of_the_right_size() {
        let dir = std::env::temp_dir().join(format!("disccraft-ac3enc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // One second of a 1 kHz tone, stereo.
        let samples: Vec<i16> = (0..48_000)
            .flat_map(|n| {
                let v = (f64::from(n) * 2.0 * std::f64::consts::PI * 1000.0 / 48_000.0).sin();
                let s = (v * 8000.0) as i16;
                [s, s]
            })
            .collect();
        let input = dir.join("tone.wav");
        let output = dir.join("tone.ac3");
        std::fs::write(&input, wav(2, &samples)).unwrap();
        let encoded = encode_wav(&input, &output).unwrap();
        let data = std::fs::read(&output).unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(encoded.channels, 2);
        assert_eq!(encoded.kbps, STEREO_KBPS);
        // 48 000 − 256 samples, then the encoder's own 256: 32 frames (the last one partly the source).
        assert_eq!(encoded.frames, 32);
        // 192 kbit/s at 48 kHz: 768 bytes a frame, each starting with the sync word.
        assert_eq!(data.len(), 32 * 768);
        for frame in data.chunks(768) {
            assert_eq!(&frame[0..2], &[0x0B, 0x77]);
        }
    }
}
