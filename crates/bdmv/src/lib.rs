//! index.bdmv / MovieObject.bdmv / MPLS / CLPI の読み書き。
//!
//! 層: L0（docs/architecture.md §5）。資料は crate の README を参照。
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod clpi;
pub mod index;
pub mod mobj;
pub mod mpls;
pub mod outline;

use std::fmt;

/// Error raised while writing or reading a BDMV file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BdmvError(pub String);

impl fmt::Display for BdmvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BdmvError {}

/// Version string written after the type indicator of every file ("0200").
pub const VERSION: &[u8; 4] = b"0200";

/// Coding of one elementary stream: `stream_coding_type` and its attributes.
///
/// The attribute codes are the ones of the BD-ROM application format:
/// `video_format` 1 = 480i, 2 = 576i, 3 = 480p, 4 = 1080i, 5 = 720p, 6 = 1080p, 7 = 576p;
/// `frame_rate` 1 = 23.976, 2 = 24, 3 = 25, 4 = 29.97, 6 = 50, 7 = 59.94;
/// `aspect` 2 = 4:3, 3 = 16:9;
/// `audio_format` 1 = mono, 3 = stereo, 6 = multi-channel;
/// `sample_rate` 1 = 48 kHz, 4 = 96 kHz, 5 = 192 kHz;
/// `language` is an ISO 639-2 code such as `*b"jpn"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamCoding {
    /// MPEG-2 video (`stream_coding_type` 0x02).
    Mpeg2Video {
        /// Video format code.
        video_format: u8,
        /// Frame rate code.
        frame_rate: u8,
        /// Aspect ratio code.
        aspect: u8,
    },
    /// H.264 / AVC video (`stream_coding_type` 0x1B).
    H264Video {
        /// Video format code.
        video_format: u8,
        /// Frame rate code.
        frame_rate: u8,
        /// Aspect ratio code.
        aspect: u8,
    },
    /// LPCM audio (`stream_coding_type` 0x80).
    Lpcm {
        /// Audio format (channel layout) code.
        audio_format: u8,
        /// Sample rate code.
        sample_rate: u8,
        /// ISO 639-2 language code.
        language: [u8; 3],
    },
    /// AC-3 audio (`stream_coding_type` 0x81).
    Ac3 {
        /// Audio format (channel layout) code.
        audio_format: u8,
        /// Sample rate code.
        sample_rate: u8,
        /// ISO 639-2 language code.
        language: [u8; 3],
    },
    /// DTS audio (`stream_coding_type` 0x82, M6.348): its core frames.
    Dts {
        /// Audio format (channel layout) code.
        audio_format: u8,
        /// Sample rate code.
        sample_rate: u8,
        /// ISO 639-2 language code.
        language: [u8; 3],
    },
    /// Presentation graphics (`stream_coding_type` 0x90).
    Pgs {
        /// ISO 639-2 language code.
        language: [u8; 3],
    },
    /// Interactive graphics (`stream_coding_type` 0x91).
    Igs {
        /// ISO 639-2 language code.
        language: [u8; 3],
    },
}

impl StreamCoding {
    /// Return the `stream_coding_type` byte.
    #[must_use]
    pub fn coding_type(&self) -> u8 {
        match self {
            Self::Mpeg2Video { .. } => 0x02,
            Self::H264Video { .. } => 0x1B,
            Self::Lpcm { .. } => 0x80,
            Self::Ac3 { .. } => 0x81,
            Self::Dts { .. } => 0x82,
            Self::Pgs { .. } => 0x90,
            Self::Igs { .. } => 0x91,
        }
    }

    /// The stream's language (ISO 639-2); `None` for video.
    #[must_use]
    pub fn language(&self) -> Option<[u8; 3]> {
        match self {
            Self::Mpeg2Video { .. } | Self::H264Video { .. } => None,
            Self::Lpcm { language, .. }
            | Self::Ac3 { language, .. }
            | Self::Dts { language, .. }
            | Self::Pgs { language, .. }
            | Self::Igs { language, .. } => Some(*language),
        }
    }
}

/// The languages of ISO 639-1 with their ISO 639-2 codes (the terminology code, and the bibliographic one where it
/// differs), from the ISO 639-2 code list of the Library of Congress (the registration authority). A Blu-ray names a
/// language with the ISO 639-2 code, a DVD with the ISO 639-1 one; media files use either (M6.421).
const LANGUAGES: &[(&str, &str, &str)] = &[
    ("aa", "aar", ""),
    ("ab", "abk", ""),
    ("af", "afr", ""),
    ("ak", "aka", ""),
    ("am", "amh", ""),
    ("ar", "ara", ""),
    ("as", "asm", ""),
    ("ay", "aym", ""),
    ("az", "aze", ""),
    ("ba", "bak", ""),
    ("be", "bel", ""),
    ("bg", "bul", ""),
    ("bn", "ben", ""),
    ("bo", "bod", "tib"),
    ("br", "bre", ""),
    ("bs", "bos", ""),
    ("ca", "cat", ""),
    ("cs", "ces", "cze"),
    ("cy", "cym", "wel"),
    ("da", "dan", ""),
    ("de", "deu", "ger"),
    ("el", "ell", "gre"),
    ("en", "eng", ""),
    ("eo", "epo", ""),
    ("es", "spa", ""),
    ("et", "est", ""),
    ("eu", "eus", "baq"),
    ("fa", "fas", "per"),
    ("fi", "fin", ""),
    ("fj", "fij", ""),
    ("fo", "fao", ""),
    ("fr", "fra", "fre"),
    ("fy", "fry", ""),
    ("ga", "gle", ""),
    ("gd", "gla", ""),
    ("gl", "glg", ""),
    ("gn", "grn", ""),
    ("gu", "guj", ""),
    ("ha", "hau", ""),
    ("he", "heb", ""),
    ("hi", "hin", ""),
    ("hr", "hrv", ""),
    ("hu", "hun", ""),
    ("hy", "hye", "arm"),
    ("id", "ind", ""),
    ("ig", "ibo", ""),
    ("is", "isl", "ice"),
    ("it", "ita", ""),
    ("iu", "iku", ""),
    ("ja", "jpn", ""),
    ("jv", "jav", ""),
    ("ka", "kat", "geo"),
    ("kk", "kaz", ""),
    ("km", "khm", ""),
    ("kn", "kan", ""),
    ("ko", "kor", ""),
    ("ku", "kur", ""),
    ("ky", "kir", ""),
    ("la", "lat", ""),
    ("lb", "ltz", ""),
    ("lo", "lao", ""),
    ("lt", "lit", ""),
    ("lv", "lav", ""),
    ("mg", "mlg", ""),
    ("mi", "mri", "mao"),
    ("mk", "mkd", "mac"),
    ("ml", "mal", ""),
    ("mn", "mon", ""),
    ("mr", "mar", ""),
    ("ms", "msa", "may"),
    ("mt", "mlt", ""),
    ("my", "mya", "bur"),
    ("nb", "nob", ""),
    ("ne", "nep", ""),
    ("nl", "nld", "dut"),
    ("nn", "nno", ""),
    ("no", "nor", ""),
    ("pa", "pan", ""),
    ("pl", "pol", ""),
    ("ps", "pus", ""),
    ("pt", "por", ""),
    ("qu", "que", ""),
    ("rm", "roh", ""),
    ("ro", "ron", "rum"),
    ("ru", "rus", ""),
    ("rw", "kin", ""),
    ("sa", "san", ""),
    ("sd", "snd", ""),
    ("si", "sin", ""),
    ("sk", "slk", "slo"),
    ("sl", "slv", ""),
    ("sm", "smo", ""),
    ("sn", "sna", ""),
    ("so", "som", ""),
    ("sq", "sqi", "alb"),
    ("sr", "srp", ""),
    ("st", "sot", ""),
    ("su", "sun", ""),
    ("sv", "swe", ""),
    ("sw", "swa", ""),
    ("ta", "tam", ""),
    ("te", "tel", ""),
    ("tg", "tgk", ""),
    ("th", "tha", ""),
    ("ti", "tir", ""),
    ("tk", "tuk", ""),
    ("tl", "tgl", ""),
    ("tn", "tsn", ""),
    ("to", "ton", ""),
    ("tr", "tur", ""),
    ("ts", "tso", ""),
    ("tt", "tat", ""),
    ("ug", "uig", ""),
    ("uk", "ukr", ""),
    ("ur", "urd", ""),
    ("uz", "uzb", ""),
    ("vi", "vie", ""),
    ("wo", "wol", ""),
    ("xh", "xho", ""),
    ("yi", "yid", ""),
    ("yo", "yor", ""),
    ("zh", "zho", "chi"),
    ("zu", "zul", ""),
];

/// The ISO 639-2 code a Blu-ray names a language with, for an ISO 639-1 one (`ja` → `jpn`, `fr` → `fra`); `und`
/// for one it does not know, and for none.
#[must_use]
pub fn iso639_2(language: Option<[u8; 2]>) -> [u8; 3] {
    language
        .and_then(|code| {
            LANGUAGES
                .iter()
                .find(|(one, _, _)| one.as_bytes() == code.as_slice())
        })
        .and_then(|(_, two, _)| <[u8; 3]>::try_from(two.as_bytes()).ok())
        .unwrap_or(*b"und")
}

/// The ISO 639-1 code (what a DVD and DiscCraft's tracks name a language with) of a language as a media file names
/// it (M6.421): an ISO 639-1 code, an ISO 639-2 one (terminology or bibliographic: `jpn`, `fre`), or a BCP 47 tag
/// (`ja-JP`, by its first part), in any case. `None` for `und`, `mul`, `zxx`, `mis` and codes not in the table.
#[must_use]
pub fn iso639_1(language: &str) -> Option<&'static str> {
    let code = language
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|(one, two, bibliographic)| {
            *one == code || *two == code || (!bibliographic.is_empty() && *bibliographic == code)
        })
        .map(|(one, _, _)| *one)
}

/// One elementary stream of a clip: its PID and coding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stream {
    /// Transport stream PID.
    pub pid: u16,
    /// Coding and attributes.
    pub coding: StreamCoding,
}

/// Convert a byte length to `u32`, failing instead of truncating.
pub(crate) fn len_u32(len: usize) -> Result<u32, BdmvError> {
    u32::try_from(len).map_err(|_| BdmvError(format!("length {len} does not fit in 32 bits")))
}

/// Overwrite the big-endian `u32` at `at`.
pub(crate) fn patch_u32(buf: &mut [u8], at: usize, value: u32) -> Result<(), BdmvError> {
    let end = at
        .checked_add(4)
        .ok_or_else(|| BdmvError("patch offset overflow".to_string()))?;
    let slot = buf
        .get_mut(at..end)
        .ok_or_else(|| BdmvError(format!("patch offset {at} out of range")))?;
    slot.copy_from_slice(&value.to_be_bytes());
    Ok(())
}

/// Big-endian reader over a byte slice that reports truncation as an error.
#[derive(Debug, Clone)]
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Start reading `data` at `pos`.
    pub(crate) fn at(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }

    /// Current position.
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    /// Read `n` bytes.
    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8], BdmvError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| BdmvError("read offset overflow".to_string()))?;
        let out = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| BdmvError(format!("truncated at byte {}", self.pos)))?;
        self.pos = end;
        Ok(out)
    }

    /// Skip `n` bytes.
    pub(crate) fn skip(&mut self, n: usize) -> Result<(), BdmvError> {
        self.bytes(n).map(|_| ())
    }

    /// Read one byte.
    pub(crate) fn u8(&mut self) -> Result<u8, BdmvError> {
        let b = self.bytes(1)?;
        b.first()
            .copied()
            .ok_or_else(|| BdmvError("truncated".to_string()))
    }

    /// Read a big-endian `u16`.
    pub(crate) fn u16(&mut self) -> Result<u16, BdmvError> {
        let b = self.bytes(2)?;
        let arr: [u8; 2] = b
            .try_into()
            .map_err(|_| BdmvError("truncated".to_string()))?;
        Ok(u16::from_be_bytes(arr))
    }

    /// Read a big-endian `u32`.
    pub(crate) fn u32(&mut self) -> Result<u32, BdmvError> {
        let b = self.bytes(4)?;
        let arr: [u8; 4] = b
            .try_into()
            .map_err(|_| BdmvError("truncated".to_string()))?;
        Ok(u32::from_be_bytes(arr))
    }
}

#[cfg(test)]
mod tests {
    use super::{LANGUAGES, Reader, iso639_1, iso639_2, patch_u32};

    /// M6.421: languages by either code, both ways; the table has no code twice.
    #[test]
    fn languages_are_found_by_any_of_their_codes() {
        assert_eq!(iso639_2(Some(*b"th")), *b"tha");
        assert_eq!(iso639_2(Some(*b"fr")), *b"fra");
        assert_eq!(iso639_2(Some(*b"xy")), *b"und");
        for (code, expected) in [
            ("jpn", Some("ja")),
            ("ENG", Some("en")),
            ("fre", Some("fr")),
            ("fra", Some("fr")),
            ("ger", Some("de")),
            ("chi", Some("zh")),
            ("ja-JP", Some("ja")),
            ("zh_Hant", Some("zh")),
            ("ko", Some("ko")),
            ("und", None),
            ("mul", None),
            ("", None),
            ("qaa", None),
        ] {
            assert_eq!(iso639_1(code), expected, "{code}");
        }
        let mut codes: Vec<&str> = LANGUAGES
            .iter()
            .flat_map(|(one, two, bibliographic)| [*one, *two, *bibliographic])
            .filter(|code| !code.is_empty())
            .collect();
        let all = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), all, "a code appears twice");
        for (one, two, _) in LANGUAGES {
            assert_eq!(iso639_1(two), Some(*one));
        }
    }

    #[test]
    fn reader_reports_truncation() {
        let data = [1, 2, 3];
        let mut r = Reader::at(&data, 0);
        assert_eq!(r.u16(), Ok(0x0102));
        assert!(r.u16().is_err());
    }

    #[test]
    fn patch_writes_big_endian() {
        let mut buf = vec![0; 6];
        assert!(patch_u32(&mut buf, 1, 0x0A0B_0C0D).is_ok());
        assert_eq!(buf, [0, 0x0A, 0x0B, 0x0C, 0x0D, 0]);
        assert!(patch_u32(&mut buf, 3, 1).is_err());
    }
}
