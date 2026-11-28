//! Hardware video encoding (M12.39 H.264, M12.44 HEVC; docs/adr/0002-gpu-conversion.md): the graphics card's
//! encoder through the operating system. Windows: a Media Foundation encoder MFT registered as hardware (NVIDIA, AMD
//! and Intel drivers register theirs), driven as the Microsoft Learn pages "Asynchronous MFTs", "H.264 Video
//! Encoder" and "H.265 / HEVC Video Encoder" describe: unlocked with `MF_TRANSFORM_ASYNC_UNLOCK`, the output type
//! (H.264 or HEVC, size, rate, profile, level) set before the input type (NV12, or P010 for 10-bit HEVC), the rate
//! control and GOP through `ICodecAPI`, then a frame in for each `METransformNeedInput` event and a packet out for
//! each `METransformHaveOutput`. M12.51: an interlaced format's H.264 (each frame coded as its two fields), which the
//! NVIDIA encoder MFT refuses, through NVIDIA's own interface instead ([`crate::nvenc`]). Other systems have none yet.

/// The stream a hardware encoder makes, and the pictures it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwCodec {
    /// H.264 High profile from NV12 pictures (the luma, then the chroma samples interleaved, a byte each).
    H264,
    /// HEVC Main 10 from P010 pictures (as NV12, but each sample two bytes, little-endian, its 10 bits at the top).
    HevcMain10,
}

/// What an encode asks of the hardware encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwVideoConfig {
    /// The stream to make.
    pub codec: HwCodec,
    /// Picture width and height.
    pub width: u32,
    /// Picture height.
    pub height: u32,
    /// Frame rate numerator.
    pub fps_num: u32,
    /// Frame rate denominator.
    pub fps_den: u32,
    /// Average bit rate, bit/s.
    pub bitrate: u32,
    /// Peak bit rate, bit/s.
    pub max_bitrate: u32,
    /// The decoder buffer (VBV / HRD), bits.
    pub buffer_bits: u32,
    /// Frames from one key frame to the next.
    pub gop: u32,
    /// B pictures between reference pictures.
    pub b_frames: u32,
    /// Slices a picture (a Blu-ray's 1080 lines take 4; H.264 only).
    pub slices: u32,
    /// The shape of a pixel (the VUI's sample aspect ratio).
    pub sar: (u32, u32),
    /// Constant bit rate: the average is the peak.
    pub cbr: bool,
    /// M12.51: `Some(top_first)` for an interlaced format's H.264, each frame (its two fields interleaved, line by
    /// line) coded as two field pictures, the top one first or not, through NVIDIA's encoder interface (NVENC); `None`
    /// for progressive frames.
    pub fields: Option<bool>,
}

/// One coded picture: an Annex B access unit and its times (100 ns units, as the encoder was given them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwPacket {
    /// The access unit, start code prefixed.
    pub data: Vec<u8>,
    /// Presentation time.
    pub pts: i64,
    /// Decoding time, when the encoder says it.
    pub dts: Option<i64>,
    /// Whether it starts a GOP (a clean point).
    pub key: bool,
}

/// The hardware encoders the system offers for `codec`, by name (empty without any).
#[must_use]
pub fn hardware_encoders(codec: HwCodec) -> Vec<String> {
    imp::encoder_names(codec)
}

/// The hardware H.264 encoders the system offers, by name (empty without any).
#[must_use]
pub fn hardware_h264_encoders() -> Vec<String> {
    hardware_encoders(HwCodec::H264)
}

/// Uses the system's hardware encoders for the HD transcodes' H.264 (M12.40) and HEVC (M12.44) when `on`:
/// registers [`transcode_factory`] with the transcodes, or takes it away.
pub fn use_for_transcodes(on: bool) {
    let factory: disccraft_transcode::hd::HardwareFactory = transcode_factory;
    disccraft_transcode::hd::set_hardware_encoder(on.then_some(factory));
}

/// The system's first hardware encoder for the codec asked, set up for an HD transcode's stream.
///
/// # Errors
///
/// When there is none, or it refuses the settings.
pub fn transcode_factory(
    config: &disccraft_transcode::hd::HardwareConfig,
) -> Result<Box<dyn disccraft_transcode::hd::HardwareVideo>, String> {
    let codec = match config.codec {
        disccraft_transcode::hd::HardwareCodec::H264 => HwCodec::H264,
        disccraft_transcode::hd::HardwareCodec::HevcMain10 => HwCodec::HevcMain10,
    };
    let encoder = HwVideoEncoder::open(&HwVideoConfig {
        codec,
        width: config.width,
        height: config.height,
        fps_num: config.fps_num,
        fps_den: config.fps_den,
        bitrate: config.bitrate,
        max_bitrate: config.max_bitrate,
        // One second at the peak rate.
        buffer_bits: config.max_bitrate,
        gop: config.gop,
        b_frames: 0,
        slices: config.slices,
        sar: config.sar,
        cbr: config.cbr,
        fields: config.fields,
    })?;
    Ok(Box::new(ForTranscodes(encoder)))
}

/// A [`HwVideoEncoder`] as the transcodes take one.
struct ForTranscodes(HwVideoEncoder);

impl disccraft_transcode::hd::HardwareVideo for ForTranscodes {
    fn encode(
        &mut self,
        picture: &[u8],
        pts: i64,
    ) -> Result<Vec<disccraft_transcode::hd::HardwarePacket>, String> {
        self.0.encode(picture, pts, false).map(for_transcodes)
    }

    fn finish(&mut self) -> Result<Vec<disccraft_transcode::hd::HardwarePacket>, String> {
        self.0.finish().map(for_transcodes)
    }
}

fn for_transcodes(packets: Vec<HwPacket>) -> Vec<disccraft_transcode::hd::HardwarePacket> {
    packets
        .into_iter()
        .map(|packet| disccraft_transcode::hd::HardwarePacket {
            data: packet.data,
            pts: packet.pts,
            key: packet.key,
        })
        .collect()
}

/// A hardware video encoder at work.
pub struct HwVideoEncoder {
    inner: Inner,
}

/// The system's encoder (Media Foundation), or NVIDIA's own interface for fields (M12.51).
enum Inner {
    System(imp::Encoder),
    Fields(crate::nvenc::NvencEncoder),
}

impl std::fmt::Debug for HwVideoEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HwVideoEncoder")
            .field("name", &self.name())
            .finish()
    }
}

impl HwVideoEncoder {
    /// The first hardware encoder the system offers for `config.codec`, set up for `config`; for fields
    /// (`config.fields`, H.264 only), NVIDIA's encoder through its own interface (M12.51).
    ///
    /// # Errors
    ///
    /// When there is none, or it refuses the settings.
    pub fn open(config: &HwVideoConfig) -> Result<Self, String> {
        let inner = match config.fields {
            Some(_) if config.codec != HwCodec::H264 => {
                return Err("fields are coded in H.264 only".to_string());
            }
            Some(top_first) => Inner::Fields(crate::nvenc::NvencEncoder::open(config, top_first)?),
            None => Inner::System(imp::Encoder::open(config)?),
        };
        Ok(Self { inner })
    }

    /// The encoder's name.
    #[must_use]
    pub fn name(&self) -> &str {
        match &self.inner {
            Inner::System(encoder) => encoder.name(),
            Inner::Fields(_) => "NVIDIA NVENC (fields)",
        }
    }

    /// Encodes one picture in the codec's layout ([`HwCodec`]: NV12 or P010) shown at `pts` (100 ns), a key frame
    /// when `key`: the packets the encoder has ready.
    ///
    /// # Errors
    ///
    /// When the encoder fails.
    pub fn encode(&mut self, picture: &[u8], pts: i64, key: bool) -> Result<Vec<HwPacket>, String> {
        match &mut self.inner {
            Inner::System(encoder) => encoder.encode(picture, pts, key),
            // The key frames are the GOP's (NVENC's own decision, every `gop` frames).
            Inner::Fields(encoder) => encoder.encode(picture, pts),
        }
    }

    /// The packets still held back, at the end of the stream.
    ///
    /// # Errors
    ///
    /// When the encoder fails.
    pub fn finish(&mut self) -> Result<Vec<HwPacket>, String> {
        match &mut self.inner {
            Inner::System(encoder) => encoder.finish(),
            Inner::Fields(encoder) => encoder.finish(),
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use std::mem::ManuallyDrop;

    use windows::Win32::Media::MediaFoundation::{
        CODECAPI_AVEncCommonBufferSize, CODECAPI_AVEncCommonMaxBitRate,
        CODECAPI_AVEncCommonMeanBitRate, CODECAPI_AVEncCommonQualityVsSpeed,
        CODECAPI_AVEncCommonRateControlMode, CODECAPI_AVEncMPVDefaultBPictureCount,
        CODECAPI_AVEncMPVGOPSize, CODECAPI_AVEncSliceControlMode, CODECAPI_AVEncSliceControlSize,
        CODECAPI_AVEncVideoForceKeyFrame, ICodecAPI, IMFActivate, IMFMediaEventGenerator,
        IMFMediaType, IMFSample, IMFShutdown, IMFTransform, MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS,
        METransformDrainComplete, METransformHaveOutput, METransformNeedInput,
        MF_E_NO_EVENTS_AVAILABLE, MF_E_TRANSFORM_STREAM_CHANGE, MF_EVENT_FLAG_NO_WAIT,
        MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
        MF_MT_MAJOR_TYPE, MF_MT_MPEG2_LEVEL, MF_MT_MPEG2_PROFILE, MF_MT_PIXEL_ASPECT_RATIO,
        MF_MT_SUBTYPE, MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES,
        MF_MT_YUV_MATRIX, MF_TRANSFORM_ASYNC_UNLOCK, MF_VERSION, MFCreateMediaType,
        MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video, MFNominalRange_16_235,
        MFSTARTUP_FULL, MFSampleExtension_CleanPoint, MFSampleExtension_DecodeTimestamp, MFStartup,
        MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
        MFT_FRIENDLY_NAME_Attribute, MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
        MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_END_STREAMING,
        MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
        MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
        MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFVideoFormat_H264, MFVideoFormat_HEVC,
        MFVideoFormat_NV12, MFVideoFormat_P010, MFVideoInterlace_Progressive,
        MFVideoPrimaries_BT709, MFVideoTransFunc_709, MFVideoTransferMatrix_BT709,
        eAVEncCommonRateControlMode_CBR, eAVEncCommonRateControlMode_PeakConstrainedVBR,
        eAVEncH264VLevel4_1, eAVEncH264VProfile_High, eAVEncH265VProfile_Main_420_10,
    };
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree};
    use windows::Win32::System::Variant::VARIANT;
    use windows::core::{GUID, Interface, PWSTR};

    use super::{HwCodec, HwPacket, HwVideoConfig};

    fn text(error: &windows::core::Error) -> String {
        format!("hardware video encoder: {error}")
    }

    /// What `codec` takes and makes: the input and output subtypes, the profile and the level (H.264: 4.1, as the
    /// Blu-ray's; HEVC: none, the encoder's own for the size and rates: the NVIDIA encoder refused level 4.1 / 5.1
    /// output types after a peak rate above the Main tier's, M12.44).
    fn formats(codec: HwCodec) -> (GUID, GUID, u32, Option<u32>) {
        match codec {
            HwCodec::H264 => (
                MFVideoFormat_NV12,
                MFVideoFormat_H264,
                u32::try_from(eAVEncH264VProfile_High.0).unwrap_or(100),
                Some(u32::try_from(eAVEncH264VLevel4_1.0).unwrap_or(41)),
            ),
            HwCodec::HevcMain10 => (
                MFVideoFormat_P010,
                MFVideoFormat_HEVC,
                u32::try_from(eAVEncH265VProfile_Main_420_10.0).unwrap_or(2),
                None,
            ),
        }
    }

    /// COM on this thread (a thread already in another mode keeps it) and Media Foundation.
    fn start() -> Result<(), String> {
        // SAFETY: no reserved pointer is passed; an already initialised thread answers S_FALSE or
        // RPC_E_CHANGED_MODE, either of which leaves COM usable.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // SAFETY: MFStartup only reads its two integer arguments; repeated calls are counted.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.map_err(|error| text(&error))
    }

    /// The hardware encoders for `codec` (from its input to its output subtype), the system's preferred first.
    fn activates(codec: HwCodec) -> Result<Vec<IMFActivate>, String> {
        start()?;
        let (from, to, ..) = formats(codec);
        let input = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: from,
        };
        let output = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: to,
        };
        let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        // SAFETY: the type infos live across the call, and `list` / `count` are valid out-pointers that the function
        // fills with an array it allocates with CoTaskMemAlloc.
        unsafe {
            MFTEnumEx(
                MFT_CATEGORY_VIDEO_ENCODER,
                MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
                Some(&raw const input),
                Some(&raw const output),
                &raw mut list,
                &raw mut count,
            )
        }
        .map_err(|error| text(&error))?;
        let mut found = Vec::new();
        if !list.is_null() {
            for index in 0..usize::try_from(count).unwrap_or(0) {
                // SAFETY: `list` holds `count` initialised elements; each is moved out once.
                let slot = unsafe { &mut *list.add(index) };
                if let Some(activate) = slot.take() {
                    found.push(activate);
                }
            }
            // SAFETY: the array came from CoTaskMemAlloc (MFTEnumEx) and its elements have been moved out.
            unsafe { CoTaskMemFree(Some(list.cast_const().cast())) };
        }
        Ok(found)
    }

    fn friendly_name(activate: &IMFActivate) -> String {
        let mut name = PWSTR::null();
        let mut length = 0u32;
        // SAFETY: the key is a valid GUID and the out-pointers are valid; the string is CoTaskMemAlloc'd.
        let found = unsafe {
            activate.GetAllocatedString(
                &MFT_FRIENDLY_NAME_Attribute,
                &raw mut name,
                &raw mut length,
            )
        };
        if found.is_err() || name.is_null() {
            return String::new();
        }
        // SAFETY: `name` is a NUL-terminated wide string the call allocated.
        let text = unsafe { name.to_string() }.unwrap_or_default();
        // SAFETY: freed once, with the allocator that made it.
        unsafe { CoTaskMemFree(Some(name.0.cast_const().cast())) };
        text
    }

    pub(super) fn encoder_names(codec: HwCodec) -> Vec<String> {
        activates(codec)
            .map(|list| list.iter().map(friendly_name).collect())
            .unwrap_or_default()
    }

    pub(super) struct Encoder {
        transform: IMFTransform,
        events: IMFMediaEventGenerator,
        codec: Option<ICodecAPI>,
        name: String,
        frame_duration: i64,
        provides_samples: bool,
        output_size: u32,
        /// `METransformNeedInput` events not answered with a picture yet.
        wanted: u32,
        drained: bool,
    }

    /// Marks `kind` as BT.709 video in the 16 to 235 range (the transcodes' pictures): an encoder that writes the
    /// VUI's colour description takes it from here (the NVIDIA H.264 and HEVC encoders left it out without, M12.44).
    fn mark_bt709(kind: &IMFMediaType) -> Result<(), String> {
        let marks = [
            (&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0),
            (&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0),
            (&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0),
            (&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0),
        ];
        for (key, value) in marks {
            // SAFETY: a plain COM call on a live media type with a valid key.
            unsafe { kind.SetUINT32(key, u32::try_from(value).unwrap_or(0)) }
                .map_err(|error| text(&error))?;
        }
        Ok(())
    }

    fn pack(high: u32, low: u32) -> u64 {
        (u64::from(high) << 32) | u64::from(low)
    }

    fn set_codec(codec: Option<&ICodecAPI>, key: &GUID, value: u32) {
        if let Some(codec) = codec {
            let value = VARIANT::from(value);
            // SAFETY: both pointers are valid for the call; the encoder copies the value. An encoder that does not
            // take the setting refuses it, which is not an error here (it keeps its own).
            let _ = unsafe { codec.SetValue(key, &raw const value) };
        }
    }

    impl Encoder {
        pub(super) fn name(&self) -> &str {
            &self.name
        }

        /// The first of the system's hardware encoders for `config.codec` that takes `config`.
        pub(super) fn open(config: &HwVideoConfig) -> Result<Self, String> {
            let mut refused = Vec::new();
            for activate in activates(config.codec)? {
                let name = friendly_name(&activate);
                match Self::open_with(&activate, name.clone(), config) {
                    Ok(encoder) => return Ok(encoder),
                    Err(error) => refused.push(format!("{name}: {error}")),
                }
                // SAFETY: a plain COM call on the activation object; its MFT is not used after this.
                let _ = unsafe { activate.ShutdownObject() };
            }
            Err(if refused.is_empty() {
                format!("no hardware {:?} encoder", config.codec)
            } else {
                refused.join("; ")
            })
        }

        fn open_with(
            activate: &IMFActivate,
            name: String,
            config: &HwVideoConfig,
        ) -> Result<Self, String> {
            let (input_subtype, output_subtype, profile, level) = formats(config.codec);
            // SAFETY: the activation object makes the MFT it stands for.
            let transform: IMFTransform =
                unsafe { activate.ActivateObject() }.map_err(|error| text(&error))?;
            // SAFETY: plain COM calls on live interfaces, each with valid arguments.
            unsafe {
                let attributes = transform.GetAttributes().map_err(|error| text(&error))?;
                attributes
                    .SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)
                    .map_err(|error| text(&error))?;
            }
            let codec: Option<ICodecAPI> = transform.cast().ok();
            // The NVIDIA encoder (M12.39) takes the GOP and the quality only before the types, and refuses a level
            // 4.1 output type after a buffer size (MF_E_UNSUPPORTED_D3D_TYPE): the buffer is set after them; the
            // slices are taken after them too.
            let set_rate = |codec: Option<&ICodecAPI>| {
                let mode = if config.cbr {
                    eAVEncCommonRateControlMode_CBR
                } else {
                    eAVEncCommonRateControlMode_PeakConstrainedVBR
                };
                set_codec(
                    codec,
                    &CODECAPI_AVEncCommonRateControlMode,
                    u32::try_from(mode.0).unwrap_or(1),
                );
                set_codec(codec, &CODECAPI_AVEncCommonMeanBitRate, config.bitrate);
                set_codec(codec, &CODECAPI_AVEncCommonMaxBitRate, config.max_bitrate);
                set_codec(codec, &CODECAPI_AVEncMPVGOPSize, config.gop);
                set_codec(
                    codec,
                    &CODECAPI_AVEncMPVDefaultBPictureCount,
                    config.b_frames,
                );
                set_codec(codec, &CODECAPI_AVEncCommonQualityVsSpeed, 100);
            };
            let set_after = |codec: Option<&ICodecAPI>| {
                set_codec(codec, &CODECAPI_AVEncCommonBufferSize, config.buffer_bits);
                // Slices of whole macroblock rows (mode 2): the rows shared out over the slices (H.264).
                if config.codec == HwCodec::H264 && config.slices > 1 {
                    let rows = config.height.div_ceil(16);
                    set_codec(codec, &CODECAPI_AVEncSliceControlMode, 2);
                    set_codec(
                        codec,
                        &CODECAPI_AVEncSliceControlSize,
                        rows.div_ceil(config.slices),
                    );
                }
            };
            set_rate(codec.as_ref());
            // SAFETY: media types made and filled here, handed to the MFT which copies what it keeps.
            unsafe {
                let output = MFCreateMediaType().map_err(|error| text(&error))?;
                output
                    .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                    .map_err(|error| text(&error))?;
                output
                    .SetGUID(&MF_MT_SUBTYPE, &output_subtype)
                    .map_err(|error| text(&error))?;
                output
                    .SetUINT32(&MF_MT_AVG_BITRATE, config.bitrate)
                    .map_err(|error| text(&error))?;
                output
                    .SetUINT64(&MF_MT_FRAME_SIZE, pack(config.width, config.height))
                    .map_err(|error| text(&error))?;
                output
                    .SetUINT64(&MF_MT_FRAME_RATE, pack(config.fps_num, config.fps_den))
                    .map_err(|error| text(&error))?;
                output
                    .SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(config.sar.0, config.sar.1))
                    .map_err(|error| text(&error))?;
                output
                    .SetUINT32(
                        &MF_MT_INTERLACE_MODE,
                        u32::try_from(MFVideoInterlace_Progressive.0).unwrap_or(2),
                    )
                    .map_err(|error| text(&error))?;
                output
                    .SetUINT32(&MF_MT_MPEG2_PROFILE, profile)
                    .map_err(|error| text(&error))?;
                if let Some(level) = level {
                    output
                        .SetUINT32(&MF_MT_MPEG2_LEVEL, level)
                        .map_err(|error| text(&error))?;
                }
                mark_bt709(&output)?;
                transform
                    .SetOutputType(0, &output, 0)
                    .map_err(|error| format!("output type: {}", text(&error)))?;

                // The encoder's own input type (NV12 / P010; a hardware encoder refuses one made here without its
                // attributes), with the picture's size and rate.
                let mut offered = None;
                for index in 0..64 {
                    let Ok(kind) = transform.GetInputAvailableType(0, index) else {
                        break;
                    };
                    if kind
                        .GetGUID(&MF_MT_SUBTYPE)
                        .is_ok_and(|subtype| subtype == input_subtype)
                    {
                        offered = Some(kind);
                        break;
                    }
                }
                let input = match offered {
                    Some(kind) => kind,
                    None => {
                        let made = MFCreateMediaType().map_err(|error| text(&error))?;
                        made.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                            .map_err(|error| text(&error))?;
                        made.SetGUID(&MF_MT_SUBTYPE, &input_subtype)
                            .map_err(|error| text(&error))?;
                        made
                    }
                };
                input
                    .SetUINT64(&MF_MT_FRAME_SIZE, pack(config.width, config.height))
                    .map_err(|error| text(&error))?;
                input
                    .SetUINT64(&MF_MT_FRAME_RATE, pack(config.fps_num, config.fps_den))
                    .map_err(|error| text(&error))?;
                input
                    .SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(config.sar.0, config.sar.1))
                    .map_err(|error| text(&error))?;
                input
                    .SetUINT32(
                        &MF_MT_INTERLACE_MODE,
                        u32::try_from(MFVideoInterlace_Progressive.0).unwrap_or(2),
                    )
                    .map_err(|error| text(&error))?;
                mark_bt709(&input)?;
                transform
                    .SetInputType(0, &input, 0)
                    .map_err(|error| format!("input type: {}", text(&error)))?;
            }
            set_rate(codec.as_ref());
            set_after(codec.as_ref());
            // SAFETY: plain COM calls on the live MFT.
            let info = unsafe { transform.GetOutputStreamInfo(0) }.map_err(|error| text(&error))?;
            let provides = u32::try_from(
                MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0,
            )
            .unwrap_or(0);
            let events: IMFMediaEventGenerator = transform.cast().map_err(|error| text(&error))?;
            // SAFETY: plain COM calls on the live MFT.
            unsafe {
                transform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                    .map_err(|error| text(&error))?;
                transform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                    .map_err(|error| text(&error))?;
            }
            let frame_duration = i64::from(config.fps_den)
                .saturating_mul(10_000_000)
                .checked_div(i64::from(config.fps_num.max(1)))
                .unwrap_or(0);
            Ok(Self {
                transform,
                events,
                codec,
                name,
                frame_duration,
                provides_samples: info.dwFlags & provides != 0,
                output_size: info.cbSize.max(1 << 20),
                wanted: 0,
                drained: false,
            })
        }

        /// Handles the next event (waiting for one when `wait`): `false` when none was there.
        fn next_event(&mut self, packets: &mut Vec<HwPacket>, wait: bool) -> Result<bool, String> {
            let flags = if wait {
                MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS(0)
            } else {
                MF_EVENT_FLAG_NO_WAIT
            };
            // SAFETY: a plain COM call on the live event generator.
            let event = match unsafe { self.events.GetEvent(flags) } {
                Ok(event) => event,
                Err(error) if error.code() == MF_E_NO_EVENTS_AVAILABLE => return Ok(false),
                Err(error) => return Err(text(&error)),
            };
            // SAFETY: a plain COM call on the event just received.
            let kind = unsafe { event.GetType() }.map_err(|error| text(&error))?;
            let is = |known: i32| u32::try_from(known).ok() == Some(kind);
            if is(METransformNeedInput.0) {
                self.wanted = self.wanted.saturating_add(1);
            } else if is(METransformHaveOutput.0) {
                if let Some(packet) = self.take_output()? {
                    packets.push(packet);
                }
            } else if is(METransformDrainComplete.0) {
                self.drained = true;
            }
            Ok(true)
        }

        fn take_output(&mut self) -> Result<Option<HwPacket>, String> {
            let given = if self.provides_samples {
                None
            } else {
                // SAFETY: plain Media Foundation constructors.
                let sample = unsafe { MFCreateSample() }.map_err(|error| text(&error))?;
                // SAFETY: as above; the buffer is handed to the sample, which keeps it.
                unsafe {
                    let buffer =
                        MFCreateMemoryBuffer(self.output_size).map_err(|error| text(&error))?;
                    sample.AddBuffer(&buffer).map_err(|error| text(&error))?;
                }
                Some(sample)
            };
            let mut buffer = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(given),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            }];
            let mut status = 0u32;
            // SAFETY: `buffer` is a valid one-element array whose sample (or none) the MFT fills; its fields are
            // taken out of their ManuallyDrop below whatever the result, so each reference is released once.
            let result = unsafe {
                self.transform
                    .ProcessOutput(0, &mut buffer, &raw mut status)
            };
            let [slot] = buffer;
            let sample: Option<IMFSample> = ManuallyDrop::into_inner(slot.pSample);
            drop(ManuallyDrop::into_inner(slot.pEvents));
            match result {
                Ok(()) => {}
                Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    self.renegotiate()?;
                    return Ok(None);
                }
                Err(error) => return Err(text(&error)),
            }
            let Some(sample) = sample else {
                return Ok(None);
            };
            // SAFETY: plain COM calls on the sample the encoder gave; the locked bytes are copied before Unlock.
            unsafe {
                let contiguous = sample
                    .ConvertToContiguousBuffer()
                    .map_err(|error| text(&error))?;
                let mut data: *mut u8 = std::ptr::null_mut();
                let mut length = 0u32;
                contiguous
                    .Lock(&raw mut data, None, Some(&raw mut length))
                    .map_err(|error| text(&error))?;
                let bytes = if data.is_null() {
                    Vec::new()
                } else {
                    std::slice::from_raw_parts(data, usize::try_from(length).unwrap_or(0)).to_vec()
                };
                contiguous.Unlock().map_err(|error| text(&error))?;
                let pts = sample.GetSampleTime().unwrap_or(0);
                let key = sample
                    .GetUINT32(&MFSampleExtension_CleanPoint)
                    .is_ok_and(|value| value != 0);
                let dts = sample
                    .GetUINT64(&MFSampleExtension_DecodeTimestamp)
                    .ok()
                    .and_then(|value| i64::try_from(value).ok());
                Ok(Some(HwPacket {
                    data: bytes,
                    pts,
                    dts,
                    key,
                }))
            }
        }

        /// The output type the encoder asks for after a stream change.
        fn renegotiate(&mut self) -> Result<(), String> {
            // SAFETY: plain COM calls on the live MFT.
            unsafe {
                let kind = self
                    .transform
                    .GetOutputAvailableType(0, 0)
                    .map_err(|error| text(&error))?;
                self.transform
                    .SetOutputType(0, &kind, 0)
                    .map_err(|error| text(&error))?;
                let info = self
                    .transform
                    .GetOutputStreamInfo(0)
                    .map_err(|error| text(&error))?;
                self.output_size = info.cbSize.max(1 << 20);
            }
            Ok(())
        }

        pub(super) fn encode(
            &mut self,
            picture: &[u8],
            pts: i64,
            key: bool,
        ) -> Result<Vec<HwPacket>, String> {
            let mut packets = Vec::new();
            while self.wanted == 0 {
                let _ = self.next_event(&mut packets, true)?;
            }
            if key {
                set_codec(self.codec.as_ref(), &CODECAPI_AVEncVideoForceKeyFrame, 1);
            }
            let length =
                u32::try_from(picture.len()).map_err(|_| "a picture too large".to_string())?;
            // SAFETY: the buffer is made `length` bytes long, locked for writing, filled with exactly `length`
            // bytes from `picture` and unlocked; the sample keeps the buffer, the MFT the sample.
            unsafe {
                let buffer = MFCreateMemoryBuffer(length).map_err(|error| text(&error))?;
                let mut data: *mut u8 = std::ptr::null_mut();
                let mut capacity = 0u32;
                buffer
                    .Lock(&raw mut data, Some(&raw mut capacity), None)
                    .map_err(|error| text(&error))?;
                if data.is_null() || capacity < length {
                    let _ = buffer.Unlock();
                    return Err("hardware video encoder: a short input buffer".to_string());
                }
                std::ptr::copy_nonoverlapping(picture.as_ptr(), data, picture.len());
                buffer.Unlock().map_err(|error| text(&error))?;
                buffer
                    .SetCurrentLength(length)
                    .map_err(|error| text(&error))?;
                let sample = MFCreateSample().map_err(|error| text(&error))?;
                sample.AddBuffer(&buffer).map_err(|error| text(&error))?;
                sample.SetSampleTime(pts).map_err(|error| text(&error))?;
                sample
                    .SetSampleDuration(self.frame_duration)
                    .map_err(|error| text(&error))?;
                self.transform
                    .ProcessInput(0, &sample, 0)
                    .map_err(|error| text(&error))?;
            }
            self.wanted = self.wanted.saturating_sub(1);
            while self.next_event(&mut packets, false)? {}
            Ok(packets)
        }

        pub(super) fn finish(&mut self) -> Result<Vec<HwPacket>, String> {
            let mut packets = Vec::new();
            // SAFETY: plain COM calls on the live MFT.
            unsafe {
                self.transform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
                    .map_err(|error| text(&error))?;
                self.transform
                    .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
                    .map_err(|error| text(&error))?;
            }
            while !self.drained {
                let _ = self.next_event(&mut packets, true)?;
            }
            Ok(packets)
        }
    }

    impl Drop for Encoder {
        fn drop(&mut self) {
            // SAFETY: plain COM calls on the live MFT; it is not used after Shutdown.
            unsafe {
                let _ = self
                    .transform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
                if let Ok(shutdown) = self.transform.cast::<IMFShutdown>() {
                    let _ = shutdown.Shutdown();
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{HwCodec, HwPacket, HwVideoConfig};

    pub(super) fn encoder_names(_codec: HwCodec) -> Vec<String> {
        Vec::new()
    }

    pub(super) struct Encoder;

    impl Encoder {
        pub(super) fn name(&self) -> &str {
            ""
        }

        pub(super) fn open(_config: &HwVideoConfig) -> Result<Self, String> {
            Err("hardware video encoding is on Windows only".to_string())
        }

        pub(super) fn encode(
            &mut self,
            _picture: &[u8],
            _pts: i64,
            _key: bool,
        ) -> Result<Vec<HwPacket>, String> {
            Err("hardware video encoding is on Windows only".to_string())
        }

        pub(super) fn finish(&mut self) -> Result<Vec<HwPacket>, String> {
            Ok(Vec::new())
        }
    }
}
