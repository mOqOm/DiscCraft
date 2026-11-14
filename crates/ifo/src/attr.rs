use crate::IfoError;

/// DVD video standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standard {
    /// NTSC, 29.97 fps rate marker.
    Ntsc,
    /// PAL, 25 fps rate marker.
    Pal,
}

/// M2 video attributes: MPEG-2, 4:3, 720 wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoAttr {
    /// Selected video standard.
    pub standard: Standard,
    /// 16:9 (anamorphic) instead of 4:3; automatic pan/scan is then disallowed, letterbox allowed.
    pub wide: bool,
}

impl VideoAttr {
    /// Encode as the two-byte DVD video attribute field.
    pub fn to_bytes(&self) -> [u8; 2] {
        let standard = match self.standard {
            Standard::Ntsc => 0x40,
            Standard::Pal => 0x50,
        };
        // byte 0: coding (MPEG-2) << 6 | standard << 4 | aspect << 2 | pan/scan disallowed << 1 | letterbox disallowed.
        // 16:9: no pan/scan vectors, letterbox allowed. 4:3: neither display conversion applies, both disallowed
        // (as Encore and other authoring tools write it).
        let aspect = if self.wide { 0x0C | 0x02 } else { 0x03 };
        [standard | aspect, 0x00]
    }

    /// Decode the two-byte DVD video attribute field, accepting only M2 values.
    pub fn from_bytes(b: [u8; 2]) -> Result<Self, IfoError> {
        let standard = match b[0] & 0xF0 {
            0x40 => Standard::Ntsc,
            0x50 => Standard::Pal,
            _ => return Err(IfoError("unsupported video attributes".to_string())),
        };
        let wide = match b[0] & 0x0F {
            0x00 | 0x03 => false,
            0x0E => true,
            _ => return Err(IfoError("unsupported video aspect flags".to_string())),
        };
        if b[1] != 0 {
            return Err(IfoError("unsupported video attributes".to_string()));
        }
        Ok(Self { standard, wide })
    }
}

/// Audio coding type supported by M2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCoding {
    /// AC-3.
    Ac3,
    /// LPCM.
    Lpcm,
}

/// M2 audio attributes: 48 kHz, 16-bit/LPCM or AC-3, optional ISO 639-1 language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioAttr {
    /// Audio coding type.
    pub coding: AudioCoding,
    /// Channel count, normally 1..=8.
    pub channels: u8,
    /// Optional two-byte ISO 639-1 language code.
    pub language: Option<[u8; 2]>,
}

impl AudioAttr {
    /// Encode as the eight-byte DVD audio attribute field.
    pub fn to_bytes(&self) -> [u8; 8] {
        let coding = match self.coding {
            AudioCoding::Ac3 => 0,
            AudioCoding::Lpcm => 4,
        };
        let lang_type = if self.language.is_some() { 1 } else { 0 };
        let ch = self.channels.saturating_sub(1).min(7);
        let mut out = [0u8; 8];
        out[0] = (coding << 5) | (lang_type << 2);
        out[1] = ch;
        if let Some(lang) = self.language {
            out[2] = lang[0];
            out[3] = lang[1];
        }
        out
    }

    /// Decode the eight-byte DVD audio attribute field, accepting only M2 values.
    pub fn from_bytes(b: [u8; 8]) -> Result<Self, IfoError> {
        if b[0] & 0x13 != 0 {
            return Err(IfoError("invalid audio attribute bits".to_string()));
        }
        let coding = match b[0] >> 5 {
            0 => AudioCoding::Ac3,
            4 => AudioCoding::Lpcm,
            _ => return Err(IfoError("unsupported audio coding".to_string())),
        };
        let lang_type = (b[0] >> 2) & 0x03;
        if lang_type > 1 {
            return Err(IfoError("invalid language type".to_string()));
        }
        let quant = b[1] >> 6;
        let rate = (b[1] >> 4) & 0x03;
        if quant != 0 || rate != 0 {
            return Err(IfoError("unsupported audio quant/rate".to_string()));
        }
        let channels = (b[1] & 0x07) + 1;
        let language = if lang_type == 1 {
            Some([b[2], b[3]])
        } else {
            if b[2] != 0 || b[3] != 0 {
                return Err(IfoError(
                    "language bytes present without language type".to_string(),
                ));
            }
            None
        };
        if b[4] != 0 || b[5] != 0 || b[6] != 0 || b[7] != 0 {
            return Err(IfoError(
                "non-zero reserved audio attribute bytes".to_string(),
            ));
        }
        Ok(Self {
            coding,
            channels,
            language,
        })
    }
}

/// Four-byte BCD time code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BcdTime(pub [u8; 4]);

fn bcd_byte(v: u8) -> u8 {
    (v / 10) << 4 | (v % 10)
}

fn bcd_decode(byte: u8) -> Result<u8, IfoError> {
    let hi = byte >> 4;
    let lo = byte & 0x0f;
    if hi > 9 || lo > 9 {
        return Err(IfoError("invalid BCD digit".to_string()));
    }
    Ok(hi * 10 + lo)
}

fn cap_u8(v: u64, max: u8) -> u8 {
    u8::try_from(v).unwrap_or(max)
}

impl BcdTime {
    /// Convert 90 kHz ticks to a non-drop BCD time code.
    pub fn from_90k(t: u64, standard: Standard) -> Self {
        let total_seconds = t / 90_000;
        let hours = cap_u8(total_seconds / 3_600, 99);
        let minutes = cap_u8((total_seconds / 60) % 60, 59);
        let seconds = cap_u8(total_seconds % 60, 59);

        let frame_ticks = match standard {
            Standard::Ntsc => 3_003,
            Standard::Pal => 3_600,
        };
        let max_frames = match standard {
            Standard::Ntsc => 29,
            Standard::Pal => 24,
        };
        let rate = match standard {
            Standard::Ntsc => 0b11,
            Standard::Pal => 0b01,
        };
        let frames = cap_u8((t % 90_000) / frame_ticks, max_frames);
        let frames_byte = (rate << 6) | bcd_byte(frames);
        Self([
            bcd_byte(hours),
            bcd_byte(minutes),
            bcd_byte(seconds),
            frames_byte,
        ])
    }

    /// Convert a BCD time code back to 90 kHz ticks.
    pub fn to_90k(&self) -> Result<u64, IfoError> {
        let hours = bcd_decode(self.0[0])?;
        let minutes = bcd_decode(self.0[1])?;
        let seconds = bcd_decode(self.0[2])?;

        let frame_byte = self.0[3];
        let rate = frame_byte >> 6;
        let frame_bcd = frame_byte & 0x3f;
        let hi = frame_bcd >> 4;
        let lo = frame_bcd & 0x0f;
        if hi > 9 || lo > 9 {
            return Err(IfoError("invalid frame BCD".to_string()));
        }
        let frames = hi * 10 + lo;

        let (frame_ticks, max_frames) = match rate {
            0b11 => (3_003u64, 29u8),
            0b01 => (3_600u64, 24u8),
            _ => return Err(IfoError("unsupported BCD frame rate".to_string())),
        };
        if frames > max_frames {
            return Err(IfoError("frame count out of range".to_string()));
        }

        // All factors are bounded by BCD digits, so these cannot overflow u64.
        let total_seconds = u64::from(hours) * 3_600 + u64::from(minutes) * 60 + u64::from(seconds);
        total_seconds
            .checked_mul(90_000)
            .and_then(|v| v.checked_add(u64::from(frames).checked_mul(frame_ticks)?))
            .ok_or_else(|| IfoError("BCD time overflow".to_string()))
    }
}

/// Eight-byte DVD VM command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmCommand(pub [u8; 8]);

impl VmCommand {
    /// Build `JumpTT n`.
    pub fn jump_tt(title: u8) -> Self {
        Self([0x30, 0x02, 0x00, 0x00, 0x00, title, 0x00, 0x00])
    }

    /// Build `Exit`.
    pub fn exit() -> Self {
        Self([0x30, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
    }
}
