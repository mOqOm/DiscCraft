//! M12.51: NVIDIA's own encoder interface (NVENC, `nvEncodeAPI.h` of the Video Codec SDK, docs/adr/0002-gpu-conversion.md),
//! for what Media Foundation cannot ask of the NVIDIA encoder: H.264 of an interlaced format, each frame coded as its
//! two field pictures (field encoding: NVIDIA's encoder takes it on Blackwell GPUs, SDK 13.0; the NVIDIA H.264 Encoder
//! MFT refuses interlaced output types, M12.48).
//!
//! Driven as NVIDIA's NVENC Video Encoder API Programming Guide describes, from the interface header's declarations
//! only: the driver's `nvEncodeAPI64.dll` fills the function table (`NvEncodeAPICreateInstance`), a session opens on a
//! CUDA context of the first CUDA device (the CUDA Driver API's `cuInit`, `cuDeviceGet`, `cuCtxCreate_v2` and
//! `cuCtxDestroy_v2` from the driver's `nvcuda.dll`), the encoder starts from a preset's configuration with the
//! stream's settings over it, and each frame goes through an input buffer the encoder allocated, synchronously, into
//! one bitstream buffer (no B pictures, so each frame's two fields come out at once, both access units in one packet).
//!
//! The structures are not transcribed: each is a zeroed block of its size, its fields written at the offsets the
//! header gives them ([`layout`], generated from the header by `scripts/nvenc/nvenc_layout.c`).

use crate::hw_video::{HwPacket, HwVideoConfig};

/// A GUID as the interface passes it (by value to some functions, inside structures elsewhere).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guid {
    /// The first 32 bits.
    pub data1: u32,
    /// The next 16 bits.
    pub data2: u16,
    /// The next 16 bits.
    pub data3: u16,
    /// The last 64 bits, in order.
    pub data4: [u8; 8],
}

#[cfg(windows)]
mod layout;

/// NVIDIA's encoder at work on an interlaced H.264 stream.
pub(crate) struct NvencEncoder {
    inner: imp::Encoder,
}

impl NvencEncoder {
    /// The first CUDA device's NVIDIA encoder, set up for `config` with each frame coded as its two fields, the top
    /// one first when `top_first`.
    ///
    /// # Errors
    ///
    /// When there is no NVIDIA encoder, it cannot code fields, or it refuses the settings.
    pub(crate) fn open(config: &HwVideoConfig, top_first: bool) -> Result<Self, String> {
        imp::Encoder::open(config, top_first).map(|inner| Self { inner })
    }

    /// Encodes one NV12 frame shown at `pts` (100 ns), its two fields: the access units it gives (both fields' in one
    /// packet).
    pub(crate) fn encode(&mut self, picture: &[u8], pts: i64) -> Result<Vec<HwPacket>, String> {
        self.inner.encode(picture, pts)
    }

    /// The end of the stream (nothing is held back without B pictures).
    pub(crate) fn finish(&mut self) -> Result<Vec<HwPacket>, String> {
        self.inner.finish()
    }
}

/// One of the interface's structures: a zeroed block of its size, 8-byte aligned, its fields written and read at the
/// header's offsets (little-endian, as on every Windows the encoder runs on). Safe code: the encoder only sees its
/// address.
#[cfg_attr(not(windows), allow(dead_code))]
struct Block {
    words: Vec<u64>,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl Block {
    fn new(size: usize) -> Self {
        Self {
            words: vec![0; size.div_ceil(8)],
        }
    }

    fn byte(&self, at: usize) -> u8 {
        self.words.get(at / 8).map_or(0, |word| {
            word.to_le_bytes().get(at % 8).copied().unwrap_or(0)
        })
    }

    fn put_byte(&mut self, at: usize, value: u8) {
        if let Some(word) = self.words.get_mut(at / 8) {
            let mut bytes = word.to_le_bytes();
            if let Some(byte) = bytes.get_mut(at % 8) {
                *byte = value;
            }
            *word = u64::from_le_bytes(bytes);
        }
    }

    fn put_bytes(&mut self, at: usize, bytes: &[u8]) {
        for (index, byte) in bytes.iter().enumerate() {
            self.put_byte(at.saturating_add(index), *byte);
        }
    }

    fn put_u32(&mut self, at: usize, value: u32) {
        self.put_bytes(at, &value.to_le_bytes());
    }

    fn put_u64(&mut self, at: usize, value: u64) {
        self.put_bytes(at, &value.to_le_bytes());
    }

    fn u32_at(&self, at: usize) -> u32 {
        u32::from_le_bytes(std::array::from_fn(|index| self.byte(at + index)))
    }

    fn u64_at(&self, at: usize) -> u64 {
        u64::from_le_bytes(std::array::from_fn(|index| self.byte(at + index)))
    }

    /// Sets the one-bit field at bit `bit` of the block.
    fn set_bit(&mut self, bit: usize, on: bool) {
        let at = bit / 8;
        let mask = 1u8 << (bit % 8);
        let byte = self.byte(at);
        self.put_byte(at, if on { byte | mask } else { byte & !mask });
    }

    fn put_guid(&mut self, at: usize, guid: Guid) {
        self.put_u32(at, guid.data1);
        self.put_bytes(at + 4, &guid.data2.to_le_bytes());
        self.put_bytes(at + 6, &guid.data3.to_le_bytes());
        self.put_bytes(at + 8, &guid.data4);
    }

    /// The bytes `at..at + len` (a nested structure, copied out).
    fn bytes(&self, at: usize, len: usize) -> Vec<u8> {
        (at..at.saturating_add(len))
            .map(|index| self.byte(index))
            .collect()
    }
}

/// The field coding asked of the encoder for each frame, as the interface numbers it.
#[cfg_attr(not(windows), allow(dead_code))]
fn picture_struct(top_first: bool) -> u32 {
    #[cfg(windows)]
    {
        if top_first {
            layout::NV_ENC_PIC_STRUCT_FIELD_TOP_BOTTOM
        } else {
            layout::NV_ENC_PIC_STRUCT_FIELD_BOTTOM_TOP
        }
    }
    #[cfg(not(windows))]
    {
        u32::from(top_first)
    }
}

/// The display aspect ratio of `width` x `height` pixels shaped `sar`, in lowest terms (the encoder works the VUI's
/// sample aspect ratio out from it).
fn display_aspect(width: u32, height: u32, sar: (u32, u32)) -> (u32, u32) {
    let across = u64::from(width).saturating_mul(u64::from(sar.0.max(1)));
    let down = u64::from(height).saturating_mul(u64::from(sar.1.max(1)));
    let (mut a, mut b) = (across, down);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let common = a.max(1);
    (
        u32::try_from(across / common).unwrap_or(16),
        u32::try_from(down / common).unwrap_or(9),
    )
}

/// Whether Annex B `data` holds an IDR picture's slice (NAL unit type 5, H.264 7.4.1).
#[cfg_attr(not(windows), allow(dead_code))]
fn has_idr(data: &[u8]) -> bool {
    data.windows(4)
        .any(|start| matches!(start, [0, 0, 1, header] if header & 0x1f == 5))
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use std::ffi::{CStr, c_char, c_void};

    use windows::Win32::Foundation::{FreeLibrary, HMODULE};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::core::{PCSTR, PCWSTR, s, w};

    use super::layout::{
        NV_ENC_BUFFER_FORMAT_NV12, NV_ENC_CAPS_PARAM__CAPS_TO_QUERY, NV_ENC_CAPS_PARAM__VERSION,
        NV_ENC_CAPS_PARAM_SIZE, NV_ENC_CAPS_PARAM_VER, NV_ENC_CAPS_SUPPORT_FIELD_ENCODING,
        NV_ENC_CODEC_H264_GUID, NV_ENC_CONFIG__ENCODE_CODEC_CONFIG,
        NV_ENC_CONFIG__FRAME_FIELD_MODE, NV_ENC_CONFIG__FRAME_INTERVAL_P,
        NV_ENC_CONFIG__GOP_LENGTH, NV_ENC_CONFIG__PROFILE_GUID, NV_ENC_CONFIG__RC_PARAMS,
        NV_ENC_CONFIG__VERSION, NV_ENC_CONFIG_H264__CHROMA_FORMAT_IDC,
        NV_ENC_CONFIG_H264__DISABLE_SPSPPS_BIT, NV_ENC_CONFIG_H264__ENTROPY_CODING_MODE,
        NV_ENC_CONFIG_H264__H264_VUI_PARAMETERS, NV_ENC_CONFIG_H264__IDR_PERIOD,
        NV_ENC_CONFIG_H264__LEVEL, NV_ENC_CONFIG_H264__OUTPUT_AUD_BIT,
        NV_ENC_CONFIG_H264__OUTPUT_BUFFERING_PERIOD_SEI_BIT,
        NV_ENC_CONFIG_H264__OUTPUT_PICTURE_TIMING_SEI_BIT, NV_ENC_CONFIG_H264__REPEAT_SPSPPS_BIT,
        NV_ENC_CONFIG_H264__SLICE_MODE, NV_ENC_CONFIG_H264__SLICE_MODE_DATA,
        NV_ENC_CONFIG_H264_SIZE,
        NV_ENC_CONFIG_H264_VUI_PARAMETERS__COLOUR_DESCRIPTION_PRESENT_FLAG,
        NV_ENC_CONFIG_H264_VUI_PARAMETERS__COLOUR_MATRIX,
        NV_ENC_CONFIG_H264_VUI_PARAMETERS__COLOUR_PRIMARIES,
        NV_ENC_CONFIG_H264_VUI_PARAMETERS__TRANSFER_CHARACTERISTICS,
        NV_ENC_CONFIG_H264_VUI_PARAMETERS__VIDEO_FORMAT,
        NV_ENC_CONFIG_H264_VUI_PARAMETERS__VIDEO_FULL_RANGE_FLAG,
        NV_ENC_CONFIG_H264_VUI_PARAMETERS__VIDEO_SIGNAL_TYPE_PRESENT_FLAG,
        NV_ENC_CONFIG_H264_VUI_PARAMETERS_SIZE, NV_ENC_CONFIG_SIZE, NV_ENC_CONFIG_VER,
        NV_ENC_CREATE_BITSTREAM_BUFFER__BITSTREAM_BUFFER, NV_ENC_CREATE_BITSTREAM_BUFFER__VERSION,
        NV_ENC_CREATE_BITSTREAM_BUFFER_SIZE, NV_ENC_CREATE_BITSTREAM_BUFFER_VER,
        NV_ENC_CREATE_INPUT_BUFFER__BUFFER_FMT, NV_ENC_CREATE_INPUT_BUFFER__HEIGHT,
        NV_ENC_CREATE_INPUT_BUFFER__INPUT_BUFFER, NV_ENC_CREATE_INPUT_BUFFER__MEMORY_HEAP,
        NV_ENC_CREATE_INPUT_BUFFER__VERSION, NV_ENC_CREATE_INPUT_BUFFER__WIDTH,
        NV_ENC_CREATE_INPUT_BUFFER_SIZE, NV_ENC_CREATE_INPUT_BUFFER_VER, NV_ENC_DEVICE_TYPE_CUDA,
        NV_ENC_ERR_NEED_MORE_INPUT, NV_ENC_H264_ENTROPY_CODING_MODE_CABAC,
        NV_ENC_H264_PROFILE_HIGH_GUID, NV_ENC_INITIALIZE_PARAMS__DAR_HEIGHT,
        NV_ENC_INITIALIZE_PARAMS__DAR_WIDTH, NV_ENC_INITIALIZE_PARAMS__ENABLE_ENCODE_ASYNC,
        NV_ENC_INITIALIZE_PARAMS__ENABLE_PTD, NV_ENC_INITIALIZE_PARAMS__ENCODE_CONFIG,
        NV_ENC_INITIALIZE_PARAMS__ENCODE_GUID, NV_ENC_INITIALIZE_PARAMS__ENCODE_HEIGHT,
        NV_ENC_INITIALIZE_PARAMS__ENCODE_WIDTH, NV_ENC_INITIALIZE_PARAMS__FRAME_RATE_DEN,
        NV_ENC_INITIALIZE_PARAMS__FRAME_RATE_NUM, NV_ENC_INITIALIZE_PARAMS__MAX_ENCODE_HEIGHT,
        NV_ENC_INITIALIZE_PARAMS__MAX_ENCODE_WIDTH, NV_ENC_INITIALIZE_PARAMS__PRESET_GUID,
        NV_ENC_INITIALIZE_PARAMS__TUNING_INFO, NV_ENC_INITIALIZE_PARAMS__VERSION,
        NV_ENC_INITIALIZE_PARAMS_SIZE, NV_ENC_INITIALIZE_PARAMS_VER, NV_ENC_LEVEL_H264_41,
        NV_ENC_LOCK_BITSTREAM__BITSTREAM_BUFFER_PTR,
        NV_ENC_LOCK_BITSTREAM__BITSTREAM_SIZE_IN_BYTES, NV_ENC_LOCK_BITSTREAM__OUTPUT_BITSTREAM,
        NV_ENC_LOCK_BITSTREAM__OUTPUT_TIME_STAMP, NV_ENC_LOCK_BITSTREAM__PICTURE_TYPE,
        NV_ENC_LOCK_BITSTREAM__VERSION, NV_ENC_LOCK_BITSTREAM_SIZE, NV_ENC_LOCK_BITSTREAM_VER,
        NV_ENC_LOCK_INPUT_BUFFER__BUFFER_DATA_PTR, NV_ENC_LOCK_INPUT_BUFFER__INPUT_BUFFER,
        NV_ENC_LOCK_INPUT_BUFFER__PITCH, NV_ENC_LOCK_INPUT_BUFFER__VERSION,
        NV_ENC_LOCK_INPUT_BUFFER_SIZE, NV_ENC_LOCK_INPUT_BUFFER_VER, NV_ENC_MEMORY_HEAP_AUTOSELECT,
        NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS__API_VERSION,
        NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS__DEVICE,
        NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS__DEVICE_TYPE,
        NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS__VERSION, NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_SIZE,
        NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER, NV_ENC_PARAMS_FRAME_FIELD_MODE_FIELD,
        NV_ENC_PARAMS_RC_CBR, NV_ENC_PARAMS_RC_VBR, NV_ENC_PIC_FLAG_EOS,
        NV_ENC_PIC_PARAMS__BUFFER_FMT, NV_ENC_PIC_PARAMS__ENCODE_PIC_FLAGS,
        NV_ENC_PIC_PARAMS__FRAME_IDX, NV_ENC_PIC_PARAMS__INPUT_BUFFER,
        NV_ENC_PIC_PARAMS__INPUT_HEIGHT, NV_ENC_PIC_PARAMS__INPUT_PITCH,
        NV_ENC_PIC_PARAMS__INPUT_TIME_STAMP, NV_ENC_PIC_PARAMS__INPUT_WIDTH,
        NV_ENC_PIC_PARAMS__OUTPUT_BITSTREAM, NV_ENC_PIC_PARAMS__PICTURE_STRUCT,
        NV_ENC_PIC_PARAMS__VERSION, NV_ENC_PIC_PARAMS_SIZE, NV_ENC_PIC_PARAMS_VER,
        NV_ENC_PIC_TYPE_I, NV_ENC_PIC_TYPE_IDR, NV_ENC_PRESET_CONFIG__PRESET_CFG,
        NV_ENC_PRESET_CONFIG__VERSION, NV_ENC_PRESET_CONFIG_SIZE, NV_ENC_PRESET_CONFIG_VER,
        NV_ENC_PRESET_P5_GUID, NV_ENC_RC_PARAMS__AVERAGE_BIT_RATE, NV_ENC_RC_PARAMS__MAX_BIT_RATE,
        NV_ENC_RC_PARAMS__RATE_CONTROL_MODE, NV_ENC_RC_PARAMS__VBV_BUFFER_SIZE,
        NV_ENC_RC_PARAMS__VBV_INITIAL_DELAY, NV_ENC_RC_PARAMS__VERSION, NV_ENC_RC_PARAMS_SIZE,
        NV_ENC_RC_PARAMS_VER, NV_ENC_SUCCESS, NV_ENC_TUNING_INFO_HIGH_QUALITY,
        NV_ENC_VUI_COLOR_PRIMARIES_BT709, NV_ENC_VUI_MATRIX_COEFFS_BT709,
        NV_ENC_VUI_TRANSFER_CHARACTERISTIC_BT709, NV_ENC_VUI_VIDEO_FORMAT_UNSPECIFIED,
        NV_ENCODE_API_FUNCTION_LIST__NV_ENC_CREATE_BITSTREAM_BUFFER,
        NV_ENCODE_API_FUNCTION_LIST__NV_ENC_CREATE_INPUT_BUFFER,
        NV_ENCODE_API_FUNCTION_LIST__NV_ENC_DESTROY_BITSTREAM_BUFFER,
        NV_ENCODE_API_FUNCTION_LIST__NV_ENC_DESTROY_ENCODER,
        NV_ENCODE_API_FUNCTION_LIST__NV_ENC_DESTROY_INPUT_BUFFER,
        NV_ENCODE_API_FUNCTION_LIST__NV_ENC_ENCODE_PICTURE,
        NV_ENCODE_API_FUNCTION_LIST__NV_ENC_GET_ENCODE_CAPS,
        NV_ENCODE_API_FUNCTION_LIST__NV_ENC_GET_ENCODE_PRESET_CONFIG_EX,
        NV_ENCODE_API_FUNCTION_LIST__NV_ENC_GET_LAST_ERROR_STRING,
        NV_ENCODE_API_FUNCTION_LIST__NV_ENC_INITIALIZE_ENCODER,
        NV_ENCODE_API_FUNCTION_LIST__NV_ENC_LOCK_BITSTREAM,
        NV_ENCODE_API_FUNCTION_LIST__NV_ENC_LOCK_INPUT_BUFFER,
        NV_ENCODE_API_FUNCTION_LIST__NV_ENC_OPEN_ENCODE_SESSION_EX,
        NV_ENCODE_API_FUNCTION_LIST__NV_ENC_UNLOCK_BITSTREAM,
        NV_ENCODE_API_FUNCTION_LIST__NV_ENC_UNLOCK_INPUT_BUFFER,
        NV_ENCODE_API_FUNCTION_LIST__VERSION, NV_ENCODE_API_FUNCTION_LIST_SIZE,
        NV_ENCODE_API_FUNCTION_LIST_VER, NVENCAPI_VERSION,
    };
    use super::{Block, Guid, display_aspect, has_idr, picture_struct};
    use crate::hw_video::{HwPacket, HwVideoConfig};

    // The nested structures fit where the header puts them.
    const _: () = assert!(
        NV_ENC_CONFIG__RC_PARAMS + NV_ENC_RC_PARAMS_SIZE <= NV_ENC_CONFIG__ENCODE_CODEC_CONFIG
    );
    const _: () =
        assert!(NV_ENC_CONFIG__ENCODE_CODEC_CONFIG + NV_ENC_CONFIG_H264_SIZE <= NV_ENC_CONFIG_SIZE);
    const _: () = assert!(
        NV_ENC_CONFIG_H264__H264_VUI_PARAMETERS + NV_ENC_CONFIG_H264_VUI_PARAMETERS_SIZE
            <= NV_ENC_CONFIG_H264_SIZE
    );
    const _: () =
        assert!(NV_ENC_PRESET_CONFIG__PRESET_CFG + NV_ENC_CONFIG_SIZE <= NV_ENC_PRESET_CONFIG_SIZE);

    type Status = u32;
    type CreateInstance = unsafe extern "system" fn(*mut c_void) -> Status;
    type MaxVersion = unsafe extern "system" fn(*mut u32) -> Status;
    type OpenSession = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> Status;
    type GetCaps = unsafe extern "system" fn(*mut c_void, Guid, *mut c_void, *mut i32) -> Status;
    type GetPreset = unsafe extern "system" fn(*mut c_void, Guid, Guid, u32, *mut c_void) -> Status;
    type WithParams = unsafe extern "system" fn(*mut c_void, *mut c_void) -> Status;
    type Destroy = unsafe extern "system" fn(*mut c_void) -> Status;
    type LastError = unsafe extern "system" fn(*mut c_void) -> *const c_char;
    type CuInit = unsafe extern "system" fn(u32) -> i32;
    type CuDeviceGet = unsafe extern "system" fn(*mut i32, i32) -> i32;
    type CuCtxCreate = unsafe extern "system" fn(*mut *mut c_void, u32, i32) -> i32;
    type CuCtxDestroy = unsafe extern "system" fn(*mut c_void) -> i32;

    /// The interface's functions this encoder calls, from the table the driver filled.
    struct Functions {
        open: OpenSession,
        caps: GetCaps,
        preset: GetPreset,
        initialize: WithParams,
        create_input: WithParams,
        destroy_input: WithParams,
        create_bitstream: WithParams,
        destroy_bitstream: WithParams,
        encode: WithParams,
        lock_bitstream: WithParams,
        unlock_bitstream: WithParams,
        lock_input: WithParams,
        unlock_input: WithParams,
        destroy: Destroy,
        last_error: LastError,
    }

    /// A loaded library, freed when dropped.
    struct Library(HMODULE);

    impl Library {
        fn load(name: PCWSTR, shown: &str) -> Result<Self, String> {
            // SAFETY: `name` is a NUL-terminated wide string literal; loading a system library runs its own start-up.
            unsafe { LoadLibraryW(name) }
                .map(Self)
                .map_err(|error| format!("NVENC: {shown} is not there ({error})"))
        }

        /// The address of the export `name` (NUL-terminated), or an error naming it.
        fn address(&self, name: PCSTR, shown: &str) -> Result<usize, String> {
            // SAFETY: the module is loaded for as long as `self` lives, and `name` is a NUL-terminated literal.
            let found = unsafe { GetProcAddress(self.0, name) };
            found
                .map(|function| function as usize)
                .ok_or_else(|| format!("NVENC: the driver has no {shown}"))
        }
    }

    impl Drop for Library {
        fn drop(&mut self) {
            // SAFETY: the module was loaded by `load` and nothing from it is called after this.
            let _ = unsafe { FreeLibrary(self.0) };
        }
    }

    /// A function pointer of type `F` at `address` (nonzero, from the driver).
    ///
    /// # Safety
    ///
    /// `address` must be a function of exactly the signature `F`.
    unsafe fn function<F: Copy>(address: usize) -> F {
        // SAFETY: the caller vouches that the address is a function of signature `F`, which is a function pointer the
        // size of a `usize` (asserted by `transmute_copy`'s caller through `F` being a fn pointer type).
        unsafe { std::mem::transmute_copy::<usize, F>(&address) }
    }

    /// The table entry at `offset` of the filled function list, as a function of type `F`.
    fn entry<F: Copy>(list: &Block, offset: usize, shown: &str) -> Result<F, String> {
        let address = usize::try_from(list.u64_at(offset)).unwrap_or(0);
        if address == 0 {
            return Err(format!("NVENC: the driver's table has no {shown}"));
        }
        // SAFETY: the driver filled this entry of NV_ENCODE_API_FUNCTION_LIST, whose type at this offset the header
        // declares with the signature `F` names (the callers pair each offset with its own type).
        Ok(unsafe { function::<F>(address) })
    }

    fn pointer(value: u64) -> *mut c_void {
        std::ptr::with_exposed_provenance_mut(usize::try_from(value).unwrap_or(0))
    }

    fn address_of(pointer: *mut c_void) -> u64 {
        u64::try_from(pointer.expose_provenance()).unwrap_or(0)
    }

    pub(crate) struct Encoder {
        functions: Functions,
        session: *mut c_void,
        input: *mut c_void,
        output: *mut c_void,
        context: *mut c_void,
        destroy_context: Option<CuCtxDestroy>,
        width: u32,
        height: u32,
        picture_struct: u32,
        frames: u32,
        // Dropped last: the functions live in these.
        _nvenc: Library,
        _cuda: Library,
    }

    impl Encoder {
        /// The driver's description of the session's last error, or `fallback`.
        fn error(&self, what: &str, status: Status) -> String {
            describe(&self.functions, self.session, what, status)
        }

        pub(crate) fn open(config: &HwVideoConfig, top_first: bool) -> Result<Self, String> {
            let nvenc = Library::load(
                w!("nvEncodeAPI64.dll"),
                "nvEncodeAPI64.dll (an NVIDIA driver)",
            )?;
            let cuda = Library::load(w!("nvcuda.dll"), "nvcuda.dll (an NVIDIA driver)")?;
            let max_version: MaxVersion = {
                let address = nvenc.address(
                    s!("NvEncodeAPIGetMaxSupportedVersion"),
                    "NvEncodeAPIGetMaxSupportedVersion",
                )?;
                // SAFETY: the export has the header's signature `NVENCSTATUS (uint32_t*)`.
                unsafe { function(address) }
            };
            let mut supported = 0u32;
            // SAFETY: the out-pointer is valid for the call.
            let status = unsafe { max_version(&raw mut supported) };
            // The driver says major << 4 | minor; this binding is for the header's version.
            let wanted = ((NVENCAPI_VERSION & 0xff) << 4) | ((NVENCAPI_VERSION >> 24) & 0xf);
            if status != NV_ENC_SUCCESS || supported < wanted {
                return Err(format!(
                    "NVENC: the NVIDIA driver offers interface {}.{}, this needs {}.{} (a newer driver)",
                    supported >> 4,
                    supported & 0xf,
                    wanted >> 4,
                    wanted & 0xf
                ));
            }
            let create: CreateInstance = {
                let address =
                    nvenc.address(s!("NvEncodeAPICreateInstance"), "NvEncodeAPICreateInstance")?;
                // SAFETY: the export has the header's signature `NVENCSTATUS (NV_ENCODE_API_FUNCTION_LIST*)`.
                unsafe { function(address) }
            };
            let mut list = Block::new(NV_ENCODE_API_FUNCTION_LIST_SIZE);
            list.put_u32(
                NV_ENCODE_API_FUNCTION_LIST__VERSION,
                NV_ENCODE_API_FUNCTION_LIST_VER,
            );
            // SAFETY: the block is the structure's size and stays alive across the call, which fills it.
            let status = unsafe { create(list.words.as_mut_ptr().cast()) };
            if status != NV_ENC_SUCCESS {
                return Err(format!(
                    "NVENC: the driver gave no functions (status {status})"
                ));
            }
            let functions = Functions {
                open: entry(
                    &list,
                    NV_ENCODE_API_FUNCTION_LIST__NV_ENC_OPEN_ENCODE_SESSION_EX,
                    "OpenEncodeSessionEx",
                )?,
                caps: entry(
                    &list,
                    NV_ENCODE_API_FUNCTION_LIST__NV_ENC_GET_ENCODE_CAPS,
                    "GetEncodeCaps",
                )?,
                preset: entry(
                    &list,
                    NV_ENCODE_API_FUNCTION_LIST__NV_ENC_GET_ENCODE_PRESET_CONFIG_EX,
                    "GetEncodePresetConfigEx",
                )?,
                initialize: entry(
                    &list,
                    NV_ENCODE_API_FUNCTION_LIST__NV_ENC_INITIALIZE_ENCODER,
                    "InitializeEncoder",
                )?,
                create_input: entry(
                    &list,
                    NV_ENCODE_API_FUNCTION_LIST__NV_ENC_CREATE_INPUT_BUFFER,
                    "CreateInputBuffer",
                )?,
                destroy_input: entry(
                    &list,
                    NV_ENCODE_API_FUNCTION_LIST__NV_ENC_DESTROY_INPUT_BUFFER,
                    "DestroyInputBuffer",
                )?,
                create_bitstream: entry(
                    &list,
                    NV_ENCODE_API_FUNCTION_LIST__NV_ENC_CREATE_BITSTREAM_BUFFER,
                    "CreateBitstreamBuffer",
                )?,
                destroy_bitstream: entry(
                    &list,
                    NV_ENCODE_API_FUNCTION_LIST__NV_ENC_DESTROY_BITSTREAM_BUFFER,
                    "DestroyBitstreamBuffer",
                )?,
                encode: entry(
                    &list,
                    NV_ENCODE_API_FUNCTION_LIST__NV_ENC_ENCODE_PICTURE,
                    "EncodePicture",
                )?,
                lock_bitstream: entry(
                    &list,
                    NV_ENCODE_API_FUNCTION_LIST__NV_ENC_LOCK_BITSTREAM,
                    "LockBitstream",
                )?,
                unlock_bitstream: entry(
                    &list,
                    NV_ENCODE_API_FUNCTION_LIST__NV_ENC_UNLOCK_BITSTREAM,
                    "UnlockBitstream",
                )?,
                lock_input: entry(
                    &list,
                    NV_ENCODE_API_FUNCTION_LIST__NV_ENC_LOCK_INPUT_BUFFER,
                    "LockInputBuffer",
                )?,
                unlock_input: entry(
                    &list,
                    NV_ENCODE_API_FUNCTION_LIST__NV_ENC_UNLOCK_INPUT_BUFFER,
                    "UnlockInputBuffer",
                )?,
                destroy: entry(
                    &list,
                    NV_ENCODE_API_FUNCTION_LIST__NV_ENC_DESTROY_ENCODER,
                    "DestroyEncoder",
                )?,
                last_error: entry(
                    &list,
                    NV_ENCODE_API_FUNCTION_LIST__NV_ENC_GET_LAST_ERROR_STRING,
                    "GetLastErrorString",
                )?,
            };
            let (context, destroy_context) = cuda_context(&cuda)?;
            let mut encoder = Self {
                functions,
                session: std::ptr::null_mut(),
                input: std::ptr::null_mut(),
                output: std::ptr::null_mut(),
                context,
                destroy_context: Some(destroy_context),
                width: config.width,
                height: config.height,
                picture_struct: picture_struct(top_first),
                frames: 0,
                _nvenc: nvenc,
                _cuda: cuda,
            };
            // From here on, dropping `encoder` frees whatever was made.
            encoder.start(config)?;
            Ok(encoder)
        }

        /// Opens the session on the CUDA context, checks the encoder codes fields, and initialises it for `config`.
        fn start(&mut self, config: &HwVideoConfig) -> Result<(), String> {
            let mut open = Block::new(NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_SIZE);
            open.put_u32(
                NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS__VERSION,
                NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER,
            );
            open.put_u32(
                NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS__DEVICE_TYPE,
                NV_ENC_DEVICE_TYPE_CUDA,
            );
            open.put_u64(
                NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS__DEVICE,
                address_of(self.context),
            );
            open.put_u32(
                NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS__API_VERSION,
                NVENCAPI_VERSION,
            );
            let mut session = std::ptr::null_mut();
            // SAFETY: the parameters block is the structure's size and the out-pointer is valid; the CUDA context it
            // names lives until the encoder is dropped.
            let status =
                unsafe { (self.functions.open)(open.words.as_mut_ptr().cast(), &raw mut session) };
            if status != NV_ENC_SUCCESS || session.is_null() {
                return Err(format!("NVENC: no encode session (status {status})"));
            }
            self.session = session;

            let mut caps = Block::new(NV_ENC_CAPS_PARAM_SIZE);
            caps.put_u32(NV_ENC_CAPS_PARAM__VERSION, NV_ENC_CAPS_PARAM_VER);
            caps.put_u32(
                NV_ENC_CAPS_PARAM__CAPS_TO_QUERY,
                NV_ENC_CAPS_SUPPORT_FIELD_ENCODING,
            );
            let mut fields = 0i32;
            // SAFETY: a live session, the parameters block the structure's size, a valid out-pointer.
            let status = unsafe {
                (self.functions.caps)(
                    self.session,
                    NV_ENC_CODEC_H264_GUID,
                    caps.words.as_mut_ptr().cast(),
                    &raw mut fields,
                )
            };
            if status != NV_ENC_SUCCESS {
                return Err(self.error("the encoder's abilities", status));
            }
            if fields < 1 {
                return Err(
                    "NVENC: this NVIDIA graphics card's encoder does not code fields (NVIDIA offers it on Blackwell \
                     cards)"
                        .to_string(),
                );
            }

            let mut preset = Block::new(NV_ENC_PRESET_CONFIG_SIZE);
            preset.put_u32(NV_ENC_PRESET_CONFIG__VERSION, NV_ENC_PRESET_CONFIG_VER);
            preset.put_u32(
                NV_ENC_PRESET_CONFIG__PRESET_CFG + NV_ENC_CONFIG__VERSION,
                NV_ENC_CONFIG_VER,
            );
            // SAFETY: a live session, the GUIDs by value, the block the structure's size.
            let status = unsafe {
                (self.functions.preset)(
                    self.session,
                    NV_ENC_CODEC_H264_GUID,
                    NV_ENC_PRESET_P5_GUID,
                    NV_ENC_TUNING_INFO_HIGH_QUALITY,
                    preset.words.as_mut_ptr().cast(),
                )
            };
            if status != NV_ENC_SUCCESS {
                return Err(self.error("the preset", status));
            }
            let mut stream = Block::new(NV_ENC_CONFIG_SIZE);
            stream.put_bytes(
                0,
                &preset.bytes(NV_ENC_PRESET_CONFIG__PRESET_CFG, NV_ENC_CONFIG_SIZE),
            );
            set_stream(&mut stream, config);

            let (dar_width, dar_height) = display_aspect(config.width, config.height, config.sar);
            let mut init = Block::new(NV_ENC_INITIALIZE_PARAMS_SIZE);
            init.put_u32(
                NV_ENC_INITIALIZE_PARAMS__VERSION,
                NV_ENC_INITIALIZE_PARAMS_VER,
            );
            init.put_guid(
                NV_ENC_INITIALIZE_PARAMS__ENCODE_GUID,
                NV_ENC_CODEC_H264_GUID,
            );
            init.put_guid(NV_ENC_INITIALIZE_PARAMS__PRESET_GUID, NV_ENC_PRESET_P5_GUID);
            init.put_u32(NV_ENC_INITIALIZE_PARAMS__ENCODE_WIDTH, config.width);
            init.put_u32(NV_ENC_INITIALIZE_PARAMS__ENCODE_HEIGHT, config.height);
            init.put_u32(NV_ENC_INITIALIZE_PARAMS__DAR_WIDTH, dar_width);
            init.put_u32(NV_ENC_INITIALIZE_PARAMS__DAR_HEIGHT, dar_height);
            init.put_u32(NV_ENC_INITIALIZE_PARAMS__FRAME_RATE_NUM, config.fps_num);
            init.put_u32(NV_ENC_INITIALIZE_PARAMS__FRAME_RATE_DEN, config.fps_den);
            init.put_u32(NV_ENC_INITIALIZE_PARAMS__ENABLE_ENCODE_ASYNC, 0);
            init.put_u32(NV_ENC_INITIALIZE_PARAMS__ENABLE_PTD, 1);
            init.put_u64(
                NV_ENC_INITIALIZE_PARAMS__ENCODE_CONFIG,
                address_of(stream.words.as_mut_ptr().cast()),
            );
            init.put_u32(NV_ENC_INITIALIZE_PARAMS__MAX_ENCODE_WIDTH, config.width);
            init.put_u32(NV_ENC_INITIALIZE_PARAMS__MAX_ENCODE_HEIGHT, config.height);
            init.put_u32(
                NV_ENC_INITIALIZE_PARAMS__TUNING_INFO,
                NV_ENC_TUNING_INFO_HIGH_QUALITY,
            );
            // SAFETY: a live session; the parameters block and the configuration it points at are the structures'
            // sizes and stay alive across the call (the encoder copies them).
            let status = unsafe {
                (self.functions.initialize)(self.session, init.words.as_mut_ptr().cast())
            };
            if status != NV_ENC_SUCCESS {
                return Err(self.error("the settings", status));
            }

            let mut input = Block::new(NV_ENC_CREATE_INPUT_BUFFER_SIZE);
            input.put_u32(
                NV_ENC_CREATE_INPUT_BUFFER__VERSION,
                NV_ENC_CREATE_INPUT_BUFFER_VER,
            );
            input.put_u32(NV_ENC_CREATE_INPUT_BUFFER__WIDTH, config.width);
            input.put_u32(NV_ENC_CREATE_INPUT_BUFFER__HEIGHT, config.height);
            input.put_u32(
                NV_ENC_CREATE_INPUT_BUFFER__MEMORY_HEAP,
                NV_ENC_MEMORY_HEAP_AUTOSELECT,
            );
            input.put_u32(
                NV_ENC_CREATE_INPUT_BUFFER__BUFFER_FMT,
                NV_ENC_BUFFER_FORMAT_NV12,
            );
            // SAFETY: a live session and a block the structure's size.
            let status = unsafe {
                (self.functions.create_input)(self.session, input.words.as_mut_ptr().cast())
            };
            if status != NV_ENC_SUCCESS {
                return Err(self.error("an input buffer", status));
            }
            self.input = pointer(input.u64_at(NV_ENC_CREATE_INPUT_BUFFER__INPUT_BUFFER));

            let mut output = Block::new(NV_ENC_CREATE_BITSTREAM_BUFFER_SIZE);
            output.put_u32(
                NV_ENC_CREATE_BITSTREAM_BUFFER__VERSION,
                NV_ENC_CREATE_BITSTREAM_BUFFER_VER,
            );
            // SAFETY: a live session and a block the structure's size.
            let status = unsafe {
                (self.functions.create_bitstream)(self.session, output.words.as_mut_ptr().cast())
            };
            if status != NV_ENC_SUCCESS {
                return Err(self.error("a bitstream buffer", status));
            }
            self.output = pointer(output.u64_at(NV_ENC_CREATE_BITSTREAM_BUFFER__BITSTREAM_BUFFER));
            if self.input.is_null() || self.output.is_null() {
                return Err("NVENC: the encoder gave no buffers".to_string());
            }
            Ok(())
        }

        pub(crate) fn encode(&mut self, picture: &[u8], pts: i64) -> Result<Vec<HwPacket>, String> {
            let (width, height) = (
                usize::try_from(self.width).unwrap_or(0),
                usize::try_from(self.height).unwrap_or(0),
            );
            let luma = width.saturating_mul(height);
            if picture.len() < luma.saturating_add(luma / 2) || width == 0 || height == 0 {
                return Err("NVENC: a picture the wrong size".to_string());
            }
            let mut lock = Block::new(NV_ENC_LOCK_INPUT_BUFFER_SIZE);
            lock.put_u32(
                NV_ENC_LOCK_INPUT_BUFFER__VERSION,
                NV_ENC_LOCK_INPUT_BUFFER_VER,
            );
            lock.put_u64(
                NV_ENC_LOCK_INPUT_BUFFER__INPUT_BUFFER,
                address_of(self.input),
            );
            // SAFETY: a live session, its own input buffer, a block the structure's size.
            let status = unsafe {
                (self.functions.lock_input)(self.session, lock.words.as_mut_ptr().cast())
            };
            if status != NV_ENC_SUCCESS {
                return Err(self.error("the input buffer", status));
            }
            let data = pointer(lock.u64_at(NV_ENC_LOCK_INPUT_BUFFER__BUFFER_DATA_PTR)).cast::<u8>();
            let pitch = usize::try_from(lock.u32_at(NV_ENC_LOCK_INPUT_BUFFER__PITCH)).unwrap_or(0);
            let copied = if data.is_null() || pitch < width {
                Err("NVENC: an input buffer the wrong shape".to_string())
            } else {
                // The NV12 planes: `height` rows of luma, then `height / 2` rows of interleaved Cb and Cr, each row at
                // the buffer's pitch, the chroma rows from `pitch * height` (as the encoder allocated it for this
                // size and format).
                for row in 0..height + height / 2 {
                    let from = row * width;
                    if let Some(source) = picture.get(from..from + width) {
                        // SAFETY: the locked buffer holds `pitch * height * 3 / 2` bytes for this NV12 size; row `row`
                        // starts at `row * pitch` and `width <= pitch` bytes fit in it. The source row is in bounds.
                        unsafe {
                            std::ptr::copy_nonoverlapping(
                                source.as_ptr(),
                                data.add(row * pitch),
                                width,
                            );
                        }
                    }
                }
                Ok(pitch)
            };
            // SAFETY: the buffer locked above, unlocked once.
            let status = unsafe { (self.functions.unlock_input)(self.session, self.input) };
            let pitch = copied?;
            if status != NV_ENC_SUCCESS {
                return Err(self.error("the input buffer", status));
            }

            let mut params = Block::new(NV_ENC_PIC_PARAMS_SIZE);
            params.put_u32(NV_ENC_PIC_PARAMS__VERSION, NV_ENC_PIC_PARAMS_VER);
            params.put_u32(NV_ENC_PIC_PARAMS__INPUT_WIDTH, self.width);
            params.put_u32(NV_ENC_PIC_PARAMS__INPUT_HEIGHT, self.height);
            params.put_u32(
                NV_ENC_PIC_PARAMS__INPUT_PITCH,
                u32::try_from(pitch).unwrap_or(self.width),
            );
            params.put_u32(NV_ENC_PIC_PARAMS__FRAME_IDX, self.frames);
            params.put_u64(
                NV_ENC_PIC_PARAMS__INPUT_TIME_STAMP,
                u64::try_from(pts).unwrap_or(0),
            );
            params.put_u64(NV_ENC_PIC_PARAMS__INPUT_BUFFER, address_of(self.input));
            params.put_u64(NV_ENC_PIC_PARAMS__OUTPUT_BITSTREAM, address_of(self.output));
            params.put_u32(NV_ENC_PIC_PARAMS__BUFFER_FMT, NV_ENC_BUFFER_FORMAT_NV12);
            params.put_u32(NV_ENC_PIC_PARAMS__PICTURE_STRUCT, self.picture_struct);
            self.frames = self.frames.wrapping_add(1);
            // SAFETY: a live session, its own buffers, a block the structure's size.
            let status =
                unsafe { (self.functions.encode)(self.session, params.words.as_mut_ptr().cast()) };
            if status == NV_ENC_ERR_NEED_MORE_INPUT {
                return Ok(Vec::new());
            }
            if status != NV_ENC_SUCCESS {
                return Err(self.error("a picture", status));
            }
            self.take_output().map(|packet| vec![packet])
        }

        /// The coded frame waiting in the bitstream buffer.
        fn take_output(&mut self) -> Result<HwPacket, String> {
            let mut lock = Block::new(NV_ENC_LOCK_BITSTREAM_SIZE);
            lock.put_u32(NV_ENC_LOCK_BITSTREAM__VERSION, NV_ENC_LOCK_BITSTREAM_VER);
            lock.put_u64(
                NV_ENC_LOCK_BITSTREAM__OUTPUT_BITSTREAM,
                address_of(self.output),
            );
            // SAFETY: a live session, its own bitstream buffer, a block the structure's size.
            let status = unsafe {
                (self.functions.lock_bitstream)(self.session, lock.words.as_mut_ptr().cast())
            };
            if status != NV_ENC_SUCCESS {
                return Err(self.error("the coded picture", status));
            }
            let data =
                pointer(lock.u64_at(NV_ENC_LOCK_BITSTREAM__BITSTREAM_BUFFER_PTR)).cast::<u8>();
            let size = usize::try_from(lock.u32_at(NV_ENC_LOCK_BITSTREAM__BITSTREAM_SIZE_IN_BYTES))
                .unwrap_or(0);
            let bytes = if data.is_null() {
                Vec::new()
            } else {
                // SAFETY: while locked, the buffer holds `size` coded bytes at `data`.
                unsafe { std::slice::from_raw_parts(data, size) }.to_vec()
            };
            let kind = lock.u32_at(NV_ENC_LOCK_BITSTREAM__PICTURE_TYPE);
            let pts =
                i64::try_from(lock.u64_at(NV_ENC_LOCK_BITSTREAM__OUTPUT_TIME_STAMP)).unwrap_or(0);
            // SAFETY: the buffer locked above, unlocked once.
            let status = unsafe { (self.functions.unlock_bitstream)(self.session, self.output) };
            if status != NV_ENC_SUCCESS {
                return Err(self.error("the coded picture", status));
            }
            if bytes.is_empty() {
                return Err("NVENC: an empty coded picture".to_string());
            }
            // The picture type the encoder reports for a frame of fields is its second field's (P after an IDR
            // field), so a frame is a key frame by its slices too.
            let key = kind == NV_ENC_PIC_TYPE_IDR || kind == NV_ENC_PIC_TYPE_I || has_idr(&bytes);
            Ok(HwPacket {
                data: bytes,
                pts,
                dts: None,
                key,
            })
        }

        pub(crate) fn finish(&mut self) -> Result<Vec<HwPacket>, String> {
            let mut params = Block::new(NV_ENC_PIC_PARAMS_SIZE);
            params.put_u32(NV_ENC_PIC_PARAMS__VERSION, NV_ENC_PIC_PARAMS_VER);
            params.put_u32(NV_ENC_PIC_PARAMS__ENCODE_PIC_FLAGS, NV_ENC_PIC_FLAG_EOS);
            // SAFETY: a live session; the end-of-stream call takes no buffers.
            let status =
                unsafe { (self.functions.encode)(self.session, params.words.as_mut_ptr().cast()) };
            if status != NV_ENC_SUCCESS {
                return Err(self.error("the end of the stream", status));
            }
            Ok(Vec::new())
        }
    }

    impl Drop for Encoder {
        fn drop(&mut self) {
            if !self.session.is_null() {
                if !self.input.is_null() {
                    // SAFETY: the session's own input buffer, destroyed once.
                    let _ = unsafe { (self.functions.destroy_input)(self.session, self.input) };
                }
                if !self.output.is_null() {
                    // SAFETY: the session's own bitstream buffer, destroyed once.
                    let _ =
                        unsafe { (self.functions.destroy_bitstream)(self.session, self.output) };
                }
                // SAFETY: the session, destroyed once, after its buffers.
                let _ = unsafe { (self.functions.destroy)(self.session) };
            }
            if let Some(destroy) = self.destroy_context.take()
                && !self.context.is_null()
            {
                // SAFETY: the context `cuda_context` made, destroyed once, after the session that used it.
                let _ = unsafe { destroy(self.context) };
            }
        }
    }

    /// The stream's settings over the preset's (`stream`, an NV_ENC_CONFIG): High profile at level 4.1 (the
    /// Blu-ray's), frame and field coding of each frame as two fields, no B pictures, the GOP and its IDR period,
    /// the rate (peak-constrained VBR or CBR) and decoder buffer, CABAC, the slices, an access unit delimiter before
    /// each picture, the parameter sets before each IDR, buffering period and picture timing SEI (the HRD a
    /// Blu-ray player checks), and BT.709 marked in the VUI.
    fn set_stream(stream: &mut Block, config: &HwVideoConfig) {
        stream.put_u32(NV_ENC_CONFIG__VERSION, NV_ENC_CONFIG_VER);
        stream.put_guid(NV_ENC_CONFIG__PROFILE_GUID, NV_ENC_H264_PROFILE_HIGH_GUID);
        stream.put_u32(NV_ENC_CONFIG__GOP_LENGTH, config.gop.max(1));
        stream.put_u32(NV_ENC_CONFIG__FRAME_INTERVAL_P, 1);
        stream.put_u32(
            NV_ENC_CONFIG__FRAME_FIELD_MODE,
            NV_ENC_PARAMS_FRAME_FIELD_MODE_FIELD,
        );
        let rate = NV_ENC_CONFIG__RC_PARAMS;
        stream.put_u32(rate + NV_ENC_RC_PARAMS__VERSION, NV_ENC_RC_PARAMS_VER);
        stream.put_u32(
            rate + NV_ENC_RC_PARAMS__RATE_CONTROL_MODE,
            if config.cbr {
                NV_ENC_PARAMS_RC_CBR
            } else {
                NV_ENC_PARAMS_RC_VBR
            },
        );
        stream.put_u32(rate + NV_ENC_RC_PARAMS__AVERAGE_BIT_RATE, config.bitrate);
        stream.put_u32(
            rate + NV_ENC_RC_PARAMS__MAX_BIT_RATE,
            config.max_bitrate.max(config.bitrate),
        );
        stream.put_u32(rate + NV_ENC_RC_PARAMS__VBV_BUFFER_SIZE, config.buffer_bits);
        stream.put_u32(
            rate + NV_ENC_RC_PARAMS__VBV_INITIAL_DELAY,
            config.buffer_bits / 10 * 9,
        );
        let h264 = NV_ENC_CONFIG__ENCODE_CODEC_CONFIG;
        stream.put_u32(h264 + NV_ENC_CONFIG_H264__LEVEL, NV_ENC_LEVEL_H264_41);
        stream.put_u32(h264 + NV_ENC_CONFIG_H264__IDR_PERIOD, config.gop.max(1));
        stream.put_u32(
            h264 + NV_ENC_CONFIG_H264__ENTROPY_CODING_MODE,
            NV_ENC_H264_ENTROPY_CODING_MODE_CABAC,
        );
        stream.put_u32(h264 + NV_ENC_CONFIG_H264__CHROMA_FORMAT_IDC, 1);
        if config.slices > 1 {
            // A number of slices a picture (mode 3).
            stream.put_u32(h264 + NV_ENC_CONFIG_H264__SLICE_MODE, 3);
            stream.put_u32(h264 + NV_ENC_CONFIG_H264__SLICE_MODE_DATA, config.slices);
        }
        let bits = h264 * 8;
        stream.set_bit(bits + NV_ENC_CONFIG_H264__OUTPUT_AUD_BIT, true);
        stream.set_bit(bits + NV_ENC_CONFIG_H264__REPEAT_SPSPPS_BIT, true);
        stream.set_bit(bits + NV_ENC_CONFIG_H264__DISABLE_SPSPPS_BIT, false);
        stream.set_bit(
            bits + NV_ENC_CONFIG_H264__OUTPUT_BUFFERING_PERIOD_SEI_BIT,
            true,
        );
        stream.set_bit(
            bits + NV_ENC_CONFIG_H264__OUTPUT_PICTURE_TIMING_SEI_BIT,
            true,
        );
        let vui = h264 + NV_ENC_CONFIG_H264__H264_VUI_PARAMETERS;
        stream.put_u32(
            vui + NV_ENC_CONFIG_H264_VUI_PARAMETERS__VIDEO_SIGNAL_TYPE_PRESENT_FLAG,
            1,
        );
        stream.put_u32(
            vui + NV_ENC_CONFIG_H264_VUI_PARAMETERS__VIDEO_FORMAT,
            NV_ENC_VUI_VIDEO_FORMAT_UNSPECIFIED,
        );
        stream.put_u32(
            vui + NV_ENC_CONFIG_H264_VUI_PARAMETERS__VIDEO_FULL_RANGE_FLAG,
            0,
        );
        stream.put_u32(
            vui + NV_ENC_CONFIG_H264_VUI_PARAMETERS__COLOUR_DESCRIPTION_PRESENT_FLAG,
            1,
        );
        stream.put_u32(
            vui + NV_ENC_CONFIG_H264_VUI_PARAMETERS__COLOUR_PRIMARIES,
            NV_ENC_VUI_COLOR_PRIMARIES_BT709,
        );
        stream.put_u32(
            vui + NV_ENC_CONFIG_H264_VUI_PARAMETERS__TRANSFER_CHARACTERISTICS,
            NV_ENC_VUI_TRANSFER_CHARACTERISTIC_BT709,
        );
        stream.put_u32(
            vui + NV_ENC_CONFIG_H264_VUI_PARAMETERS__COLOUR_MATRIX,
            NV_ENC_VUI_MATRIX_COEFFS_BT709,
        );
    }

    /// A CUDA context on the first CUDA device (the NVIDIA card), current on this thread, and the function that
    /// destroys it.
    fn cuda_context(cuda: &Library) -> Result<(*mut c_void, CuCtxDestroy), String> {
        // SAFETY: the exports have the CUDA Driver API's signatures `CUresult cuInit(unsigned int)`,
        // `cuDeviceGet(CUdevice*, int)`, `cuCtxCreate_v2(CUcontext*, unsigned int, CUdevice)` and
        // `cuCtxDestroy_v2(CUcontext)`, with `CUdevice` an int and `CUcontext` a pointer.
        let init: CuInit = unsafe { function(cuda.address(s!("cuInit"), "cuInit")?) };
        // SAFETY: as above.
        let device_get: CuDeviceGet =
            unsafe { function(cuda.address(s!("cuDeviceGet"), "cuDeviceGet")?) };
        // SAFETY: as above.
        let create: CuCtxCreate =
            unsafe { function(cuda.address(s!("cuCtxCreate_v2"), "cuCtxCreate_v2")?) };
        // SAFETY: as above.
        let destroy: CuCtxDestroy =
            unsafe { function(cuda.address(s!("cuCtxDestroy_v2"), "cuCtxDestroy_v2")?) };
        // SAFETY: cuInit takes flags 0.
        let status = unsafe { init(0) };
        if status != 0 {
            return Err(format!("NVENC: CUDA did not start (error {status})"));
        }
        let mut device = 0i32;
        // SAFETY: a valid out-pointer and the first device.
        let status = unsafe { device_get(&raw mut device, 0) };
        if status != 0 {
            return Err(format!("NVENC: no CUDA device (error {status})"));
        }
        let mut context = std::ptr::null_mut();
        // SAFETY: a valid out-pointer, no flags, the device found above.
        let status = unsafe { create(&raw mut context, 0, device) };
        if status != 0 || context.is_null() {
            return Err(format!("NVENC: no CUDA context (error {status})"));
        }
        Ok((context, destroy))
    }

    /// `what` failed with `status`: the driver's words for the session's last error when it has them.
    fn describe(functions: &Functions, session: *mut c_void, what: &str, status: Status) -> String {
        let said = if session.is_null() {
            None
        } else {
            // SAFETY: a live session; the driver returns a NUL-terminated string it owns, or null.
            let text = unsafe { (functions.last_error)(session) };
            if text.is_null() {
                None
            } else {
                // SAFETY: a NUL-terminated string from the driver, read before the next call into it.
                Some(
                    unsafe { CStr::from_ptr(text) }
                        .to_string_lossy()
                        .into_owned(),
                )
            }
        };
        match said.filter(|said| !said.trim().is_empty()) {
            Some(said) => format!("NVENC: {what}: {said} (status {status})"),
            None => format!("NVENC: {what} (status {status})"),
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use crate::hw_video::{HwPacket, HwVideoConfig};

    pub(crate) struct Encoder;

    impl Encoder {
        pub(crate) fn open(_config: &HwVideoConfig, _top_first: bool) -> Result<Self, String> {
            Err("NVENC: only on Windows here".to_string())
        }

        pub(crate) fn encode(
            &mut self,
            _picture: &[u8],
            _pts: i64,
        ) -> Result<Vec<HwPacket>, String> {
            Err("NVENC: only on Windows here".to_string())
        }

        pub(crate) fn finish(&mut self) -> Result<Vec<HwPacket>, String> {
            Ok(Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Block, display_aspect, has_idr};

    /// The structures are written field by field at byte and bit offsets: a value lands little-endian where it was
    /// put, across the 8-byte words the block is made of, and a bit is set and cleared without touching its
    /// neighbours.
    #[test]
    fn a_block_puts_fields_where_asked() {
        let mut block = Block::new(20);
        block.put_u32(6, 0x1122_3344);
        assert_eq!(block.u32_at(6), 0x1122_3344);
        assert_eq!(block.bytes(6, 4), vec![0x44, 0x33, 0x22, 0x11]);
        block.put_u64(8, u64::MAX);
        block.set_bit(8 * 8 + 3, false);
        assert_eq!(block.u64_at(8), !(1u64 << 3));
        // Nothing is written past the block's end.
        block.put_u32(30, 7);
        assert_eq!(block.words.len(), 3);
    }

    /// A frame of fields is a key frame when it holds an IDR slice (the encoder reports its second field's type).
    #[test]
    fn a_frame_with_an_idr_slice_is_a_key_frame() {
        // An access unit delimiter, then an IDR slice (0x65) / a non-IDR slice (0x41).
        assert!(has_idr(&[0, 0, 0, 1, 0x09, 0x10, 0, 0, 1, 0x65, 0x88]));
        assert!(!has_idr(&[0, 0, 0, 1, 0x09, 0x30, 0, 0, 1, 0x41, 0x9a]));
        assert!(!has_idr(&[0, 0, 1]));
    }

    /// The display aspect ratio the encoder works the pixel shape out from: 16:9 for square-pixel HD, and the same
    /// 16:9 for 1440 x 1080 of 4:3 pixels.
    #[test]
    fn the_display_aspect_follows_the_pixel_shape() {
        assert_eq!(display_aspect(1920, 1080, (1, 1)), (16, 9));
        assert_eq!(display_aspect(1440, 1080, (4, 3)), (16, 9));
        assert_eq!(display_aspect(720, 480, (40, 33)), (20, 11));
    }
}
