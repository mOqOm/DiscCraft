//! VOBU scheduling and pack emission.

use super::pack;
use super::{
    AudioSource, GopInfo, MuxError, MuxInput, SUBPICTURE_COUNT, SubpictureUnit, VobuInfo,
    VobuRecord, invalid, u32_from_i64,
};
use crate::es::{Ac3Stream, LpcmStream, VideoStream, to_90k};
use crate::sink::{SECTOR, SectorSink};

/// MPEG-2 pack header length in bytes.
const PACK_HEADER_LEN: usize = 14;
/// Base PES header length in bytes, including the three optional-header bytes.
const PES_HEADER_BASE: usize = 9;
/// Video stream id.
const VIDEO_STREAM_ID: u8 = 0xE0;
/// Private stream 1 id used by AC-3 and LPCM.
const PRIVATE_STREAM_ID: u8 = 0xBD;
/// MPEG-2 video sequence end code.
const SEQ_END: [u8; 4] = [0x00, 0x00, 0x01, 0xB7];
/// AC-3 private stream subheader length.
const AC3_SUBHEADER_LEN: usize = 4;
/// LPCM private stream subheader length.
const LPCM_SUBHEADER_LEN: usize = 7;
/// PES optional header length for PTS only.
const AUDIO_HDR_LEN: usize = 5;
/// PES optional header length for PTS only.
const PTS_ONLY_LEN: usize = 5;
/// PES optional header length for PTS and DTS.
const PTS_DTS_LEN: usize = 10;
/// PES flags2 value for PTS only.
const PES_FLAGS_PTS: u8 = 0x80;
/// PES flags2 value for PTS and DTS.
const PES_FLAGS_PTS_DTS: u8 = 0xC0;
/// PES flags2 value with no timestamps.
const PES_FLAGS_NONE: u8 = 0x00;
/// Maximum AC-3 elementary data bytes in one pack.
const AC3_DATA_CAPACITY: usize =
    SECTOR - PACK_HEADER_LEN - PES_HEADER_BASE - AUDIO_HDR_LEN - AC3_SUBHEADER_LEN;
/// Maximum LPCM sample bytes in one pack.
const LPCM_DATA_CAPACITY: usize =
    SECTOR - PACK_HEADER_LEN - PES_HEADER_BASE - AUDIO_HDR_LEN - LPCM_SUBHEADER_LEN;
/// Maximum video elementary data bytes in one pack with no PES optional header.
const VIDEO_NO_HDR_CAPACITY: usize = SECTOR - PACK_HEADER_LEN - PES_HEADER_BASE;
/// LPCM frame duration in 27 MHz units.
const LPCM_FRAME_PTS: i64 = 45_000;
/// Subpicture private stream sub stream base.
const SUB_STREAM_BASE: u8 = 0x20;
/// Length of the subpicture sub stream id byte.
const SUB_ID_LEN: usize = 1;
/// Maximum subpicture SPU bytes in a first PES packet with PTS.
const SUB_FIRST_CAPACITY: usize =
    SECTOR - PACK_HEADER_LEN - PES_HEADER_BASE - PTS_ONLY_LEN - SUB_ID_LEN;
/// Maximum subpicture SPU bytes in a continuation PES packet without PTS.
const SUB_CONT_CAPACITY: usize = SECTOR - PACK_HEADER_LEN - PES_HEADER_BASE - SUB_ID_LEN;

/// Progress state for the video payload of one GOP.
#[derive(Debug)]
struct GopState {
    /// Bytes of the GOP payload already sent.
    sent: usize,
    /// Index into `GopInfo::pic_starts` of the picture owning `sent`.
    owner_idx: usize,
    /// Index into `GopInfo::pic_starts` of the first picture start at or after `sent`.
    next_idx: usize,
}

/// Borrowed audio source kind.
#[derive(Clone, Copy, Debug)]
enum AudioKind<'a> {
    /// AC-3 stream and its elementary-stream bytes.
    Ac3 {
        /// Parsed AC-3 stream.
        stream: &'a Ac3Stream,
        /// AC-3 elementary stream bytes.
        data: &'a [u8],
    },
    /// DVD LPCM stream.
    Lpcm(&'a LpcmStream),
}

/// Scheduler state for one audio stream.
#[derive(Debug)]
pub(crate) struct AudioState<'a> {
    /// Audio source kind.
    kind: AudioKind<'a>,
    /// Next frame index to emit.
    next_frame: usize,
    /// Audio start PTS in 27 MHz units.
    start_pts: i64,
}

/// Scheduler state for one subpicture stream.
#[derive(Debug)]
pub(crate) struct SubpictureState<'a> {
    /// Subpicture units for this stream.
    units: &'a [SubpictureUnit],
    /// Next unit index to emit.
    next_unit: usize,
    /// Bytes already emitted from the current unit.
    offset: usize,
}

/// Candidate stream for the next pack.
#[derive(Debug)]
enum Candidate {
    /// Current GOP video payload.
    Video,
    /// Audio stream by index.
    Audio(usize),
    /// Subpicture stream by index.
    Subpicture(usize),
}

/// Chosen video PES header and payload length.
#[derive(Debug)]
struct VideoPlan {
    /// Payload length in bytes.
    payload_len: usize,
    /// PES optional header length.
    header_data_len: usize,
    /// PES flags2 byte.
    flags2: u8,
    /// PES optional header bytes.
    optional: Vec<u8>,
}

/// Video PES header candidate.
#[derive(Debug)]
struct VideoHeader {
    /// Relative GOP byte offset of the picture start.
    start: usize,
    /// PES optional header length.
    header_len: usize,
    /// PES flags2 byte.
    flags2: u8,
    /// PES optional header bytes.
    optional: Vec<u8>,
}

/// Initialize one scheduler state per audio source.
pub(crate) fn init_audio_states<'a>(input: &MuxInput<'a>) -> Result<Vec<AudioState<'a>>, MuxError> {
    let mut states = Vec::new();
    for source in input.audio.iter() {
        match source {
            AudioSource::Ac3 { stream, data } => {
                states.push(AudioState {
                    kind: AudioKind::Ac3 { stream, data },
                    next_frame: 0,
                    start_pts: input.audio_start_pts,
                });
            }
            AudioSource::Lpcm(lpcm) => {
                if lpcm.frame_bytes == 0 && !lpcm.data.is_empty() {
                    return Err(invalid("LPCM frame_bytes must be non-zero"));
                }
                states.push(AudioState {
                    kind: AudioKind::Lpcm(lpcm),
                    next_frame: 0,
                    start_pts: input.audio_start_pts,
                });
            }
        }
    }
    Ok(states)
}

/// Initialize one scheduler state per subpicture stream.
pub(crate) fn init_subpicture_states<'a>(
    input: &'a MuxInput<'a>,
) -> Result<Vec<SubpictureState<'a>>, MuxError> {
    if input.subpictures.len() > SUBPICTURE_COUNT {
        return Err(invalid("too many subpicture streams"));
    }

    let mut states = Vec::new();
    for units in input.subpictures.iter() {
        validate_subpicture_units(units)?;
        states.push(SubpictureState {
            units,
            next_unit: 0,
            offset: 0,
        });
    }

    Ok(states)
}

/// Validate one subpicture stream's units.
fn validate_subpicture_units(units: &[SubpictureUnit]) -> Result<(), MuxError> {
    let mut prev: Option<i64> = None;
    for unit in units.iter() {
        if unit.pts < 0 {
            return Err(invalid("negative subpicture pts"));
        }
        if unit.data.is_empty() {
            return Err(invalid("empty subpicture unit"));
        }
        if let Some(prev) = prev
            && unit.pts < prev
        {
            return Err(invalid("subpicture units must be sorted by pts"));
        }
        prev = Some(unit.pts);
    }
    Ok(())
}

/// Mux one VOBU: placeholder NAV pack, then scheduled data packs.
#[allow(clippy::too_many_arguments)]
pub(crate) fn mux_vobu(
    input: &MuxInput<'_>,
    gops: &[GopInfo],
    gop_idx: usize,
    cell_idx: usize,
    cell_id: u8,
    cell_start_ptm: u32,
    next_gop_dts: Option<i64>,
    append_seq_end: bool,
    audio_states: &mut [AudioState<'_>],
    subpicture_states: &mut [SubpictureState<'_>],
    sink: &mut dyn SectorSink,
    total_sectors: u32,
    prev_scr: i128,
    rem: i128,
    first_nav: bool,
) -> Result<(VobuRecord, u32, i128, i128), MuxError> {
    let _ = cell_id;
    let gop = gops.get(gop_idx).ok_or_else(|| invalid("missing GOP"))?;
    let nav_sector = total_sectors;

    let (nav_scr, mut rem) = if first_nav {
        (0i128, 0i128)
    } else {
        pack::next_scr(prev_scr, rem, None)?
    };

    sink.write_sector(&[0u8; SECTOR])?;
    let mut total_sectors = total_sectors
        .checked_add(1)
        .ok_or_else(|| invalid("sector count overflow"))?;
    let mut last_sector = nav_sector;
    let mut prev_scr = nav_scr;

    let mut state = GopState {
        sent: 0,
        owner_idx: 0,
        next_idx: 0,
    };
    let mut ref_last = [None, None, None];
    let mut first_audio = [None; 8];
    let mut first_subpicture = [None; SUBPICTURE_COUNT];

    let seq_end_len = if append_seq_end { SEQ_END.len() } else { 0 };
    let total_payload_len = gop
        .byte_len
        .checked_add(seq_end_len)
        .ok_or_else(|| invalid("GOP payload overflow"))?;

    while let Some((candidate, deadline)) = choose_candidate(
        gop,
        input.video,
        &state,
        total_payload_len,
        audio_states,
        subpicture_states,
        next_gop_dts,
    )? {
        let (scr, new_rem) = pack::next_scr(prev_scr, rem, Some(deadline))?;
        let sector_idx = total_sectors;
        let mut sector = [0u8; SECTOR];

        emit_pack(
            candidate,
            scr,
            sector_idx,
            gop,
            input,
            &mut state,
            total_payload_len,
            append_seq_end,
            audio_states,
            subpicture_states,
            &mut ref_last,
            &mut first_audio,
            &mut first_subpicture,
            &mut sector,
        )?;

        sink.write_sector(&sector)?;
        total_sectors = total_sectors
            .checked_add(1)
            .ok_or_else(|| invalid("sector count overflow"))?;
        last_sector = sector_idx;
        prev_scr = scr;
        rem = new_rem;
    }

    let s_ptm = u32_from_i64(to_90k(gop.display_start))?;
    let next_gop = gop_idx.checked_add(1).and_then(|idx| gops.get(idx));
    let e_ptm = if let Some(next) = next_gop {
        u32_from_i64(to_90k(next.display_start))?
    } else {
        u32_from_i64(to_90k(gop.display_end))?
    };

    let info = VobuInfo {
        start_sector: nav_sector,
        last_sector,
        s_ptm,
        e_ptm,
        cell: cell_idx,
    };
    let record = VobuRecord {
        info,
        nav_scr,
        ref_last,
        first_audio,
        first_subpicture,
        cell_start_ptm,
    };

    Ok((record, total_sectors, prev_scr, rem))
}

/// Choose the next stream to emit and its deadline.
fn choose_candidate(
    gop: &GopInfo,
    video: &VideoStream,
    state: &GopState,
    total_payload_len: usize,
    audio_states: &[AudioState<'_>],
    subpicture_states: &[SubpictureState<'_>],
    next_gop_dts: Option<i64>,
) -> Result<Option<(Candidate, i64)>, MuxError> {
    let mut best = video_deadline(gop, video, state, total_payload_len)?
        .map(|deadline| (Candidate::Video, deadline));

    if let Some(candidate) = choose_audio(audio_states, next_gop_dts)?
        && better_deadline(&best, candidate.1)
    {
        best = Some(candidate);
    }

    if let Some(candidate) = choose_subpicture(state.sent > 0, subpicture_states, next_gop_dts)?
        && better_deadline(&best, candidate.1)
    {
        best = Some(candidate);
    }

    Ok(best)
}

/// Choose the best audio candidate.
fn choose_audio(
    audio_states: &[AudioState<'_>],
    next_gop_dts: Option<i64>,
) -> Result<Option<(Candidate, i64)>, MuxError> {
    let mut best: Option<(Candidate, i64)> = None;

    for (idx, audio) in audio_states.iter().enumerate() {
        if audio.remaining_frames()? == 0 {
            continue;
        }
        let pts = audio.next_pts()?;
        if !audio_eligible(pts, next_gop_dts) {
            continue;
        }
        if better_deadline(&best, pts) {
            best = Some((Candidate::Audio(idx), pts));
        }
    }

    Ok(best)
}

/// Choose the best subpicture candidate.
fn choose_subpicture(
    video_started: bool,
    subpicture_states: &[SubpictureState<'_>],
    next_gop_dts: Option<i64>,
) -> Result<Option<(Candidate, i64)>, MuxError> {
    let mut best: Option<(Candidate, i64)> = None;
    let partial = has_partial_subpicture(subpicture_states);

    for (idx, state) in subpicture_states.iter().enumerate() {
        if !subpicture_candidate_allowed(partial, video_started, state) {
            continue;
        }
        let pts = state.next_pts()?;
        if !partial && !audio_eligible(pts, next_gop_dts) {
            continue;
        }
        if better_deadline(&best, pts) {
            best = Some((Candidate::Subpicture(idx), pts));
        }
    }

    Ok(best)
}

/// Whether any subpicture stream has a partially emitted unit.
fn has_partial_subpicture(subpicture_states: &[SubpictureState<'_>]) -> bool {
    subpicture_states.iter().any(|state| state.has_partial())
}

/// Whether a subpicture stream may be considered as a candidate.
fn subpicture_candidate_allowed(
    partial: bool,
    video_started: bool,
    state: &SubpictureState<'_>,
) -> bool {
    if partial {
        state.has_partial()
    } else {
        video_started && state.has_pending()
    }
}

/// Whether an audio frame may be muxed in the current VOBU.
fn audio_eligible(pts: i64, next_gop_dts: Option<i64>) -> bool {
    match next_gop_dts {
        Some(dts) => pts < dts,
        None => true,
    }
}

/// Whether `deadline` is strictly better than the current best deadline.
fn better_deadline(best: &Option<(Candidate, i64)>, deadline: i64) -> bool {
    match best {
        Some((_, current)) => deadline < *current,
        None => true,
    }
}

/// Deadline of the next unsent video byte.
fn video_deadline(
    gop: &GopInfo,
    video: &VideoStream,
    state: &GopState,
    total_payload_len: usize,
) -> Result<Option<i64>, MuxError> {
    if state.sent >= total_payload_len {
        return Ok(None);
    }

    if state.sent < gop.byte_len {
        let pic_idx = state.owner_idx;
        let global_idx = gop
            .start_pic
            .checked_add(pic_idx)
            .ok_or_else(|| invalid("picture index overflow"))?;
        let pic = video
            .pictures
            .get(global_idx)
            .ok_or_else(|| invalid("missing picture"))?;
        Ok(Some(pic.dts))
    } else {
        Ok(Some(gop.display_end))
    }
}

/// Emit one scheduled pack into `sector`.
#[allow(clippy::too_many_arguments)]
fn emit_pack(
    candidate: Candidate,
    scr: i128,
    sector_idx: u32,
    gop: &GopInfo,
    input: &MuxInput<'_>,
    state: &mut GopState,
    total_payload_len: usize,
    append_seq_end: bool,
    audio_states: &mut [AudioState<'_>],
    subpicture_states: &mut [SubpictureState<'_>],
    ref_last: &mut [Option<u32>; 3],
    first_audio: &mut [Option<u32>; 8],
    first_subpicture: &mut [Option<u32>; SUBPICTURE_COUNT],
    sector: &mut [u8; SECTOR],
) -> Result<(), MuxError> {
    match candidate {
        Candidate::Video => emit_video_pack(
            scr,
            sector_idx,
            gop,
            input,
            state,
            total_payload_len,
            append_seq_end,
            ref_last,
            sector,
        ),
        Candidate::Audio(stream_index) => emit_audio_pack(
            scr,
            sector_idx,
            stream_index,
            audio_states,
            first_audio,
            sector,
        ),
        Candidate::Subpicture(stream_index) => emit_subpicture_pack(
            scr,
            sector_idx,
            stream_index,
            subpicture_states,
            first_subpicture,
            sector,
        ),
    }
}

/// Emit one video pack.
#[allow(clippy::too_many_arguments)]
fn emit_video_pack(
    scr: i128,
    sector_idx: u32,
    gop: &GopInfo,
    input: &MuxInput<'_>,
    state: &mut GopState,
    total_payload_len: usize,
    append_seq_end: bool,
    ref_last: &mut [Option<u32>; 3],
    sector: &mut [u8; SECTOR],
) -> Result<(), MuxError> {
    let plan = choose_video_payload(gop, input.video, state, total_payload_len)?;
    let payload = build_video_payload(
        gop,
        input.video_data,
        append_seq_end,
        state.sent,
        plan.payload_len,
    )?;

    pack::write_pes_pack(
        sector,
        scr,
        VIDEO_STREAM_ID,
        plan.flags2,
        plan.header_data_len,
        &plan.optional,
        &payload,
    )?;

    let payload_start = state.sent;
    let payload_end = payload_start
        .checked_add(plan.payload_len)
        .ok_or_else(|| invalid("payload range overflow"))?;

    update_ref_last(
        gop,
        input.video,
        payload_start,
        payload_end,
        sector_idx,
        ref_last,
    )?;
    advance_state(gop, state, payload_end);
    Ok(())
}

/// Emit one audio pack.
fn emit_audio_pack(
    scr: i128,
    sector_idx: u32,
    stream_index: usize,
    audio_states: &mut [AudioState<'_>],
    first_audio: &mut [Option<u32>; 8],
    sector: &mut [u8; SECTOR],
) -> Result<(), MuxError> {
    let state = audio_states
        .get_mut(stream_index)
        .ok_or_else(|| invalid("missing audio state"))?;
    let (payload, pts, _count) = state.build_payload(stream_index)?;
    let optional = pack::pts_bytes(pts, pts, false)?;

    pack::write_pes_pack(
        sector,
        scr,
        PRIVATE_STREAM_ID,
        PES_FLAGS_PTS,
        AUDIO_HDR_LEN,
        &optional,
        &payload,
    )?;

    if let Some(slot) = first_audio.get_mut(stream_index)
        && slot.is_none()
    {
        *slot = Some(sector_idx);
    }
    Ok(())
}

/// Emit one subpicture pack.
fn emit_subpicture_pack(
    scr: i128,
    sector_idx: u32,
    stream_index: usize,
    subpicture_states: &mut [SubpictureState<'_>],
    first_subpicture: &mut [Option<u32>; SUBPICTURE_COUNT],
    sector: &mut [u8; SECTOR],
) -> Result<(), MuxError> {
    let state = subpicture_states
        .get_mut(stream_index)
        .ok_or_else(|| invalid("missing subpicture state"))?;
    let (payload, pts, first) = state.build_payload(stream_index)?;

    let (flags2, header_data_len, optional) = if first {
        (
            PES_FLAGS_PTS,
            PTS_ONLY_LEN,
            pack::pts_bytes(pts, pts, false)?,
        )
    } else {
        (PES_FLAGS_NONE, 0, Vec::new())
    };

    pack::write_pes_pack(
        sector,
        scr,
        PRIVATE_STREAM_ID,
        flags2,
        header_data_len,
        &optional,
        &payload,
    )?;

    if let Some(slot) = first_subpicture.get_mut(stream_index)
        && slot.is_none()
    {
        *slot = Some(sector_idx);
    }
    Ok(())
}

/// Choose video payload length and PES header.
fn choose_video_payload(
    gop: &GopInfo,
    video: &VideoStream,
    state: &GopState,
    total_payload_len: usize,
) -> Result<VideoPlan, MuxError> {
    let remaining = total_payload_len
        .checked_sub(state.sent)
        .ok_or_else(|| invalid("video payload underflow"))?;
    if remaining == 0 {
        return Err(invalid("empty video payload"));
    }

    let no_hdr_len = remaining.min(VIDEO_NO_HDR_CAPACITY);
    if let Some(pic_idx) = first_pic_start_at_or_after(gop, state)
        && let Some(header) = video_header(gop, video, state.sent, no_hdr_len, pic_idx)?
    {
        let cap = VIDEO_NO_HDR_CAPACITY
            .checked_sub(header.header_len)
            .ok_or_else(|| invalid("PES header too large"))?;
        let with_hdr_len = remaining.min(cap);
        let end = state
            .sent
            .checked_add(with_hdr_len)
            .ok_or_else(|| invalid("payload overflow"))?;
        if with_hdr_len > 0 && header.start < end {
            return Ok(VideoPlan {
                payload_len: with_hdr_len,
                header_data_len: header.header_len,
                flags2: header.flags2,
                optional: header.optional,
            });
        }
    }

    Ok(VideoPlan {
        payload_len: no_hdr_len,
        header_data_len: 0,
        flags2: PES_FLAGS_NONE,
        optional: Vec::new(),
    })
}

/// Build a video PES header if a picture starts inside the candidate payload.
fn video_header(
    gop: &GopInfo,
    video: &VideoStream,
    sent: usize,
    payload_len: usize,
    pic_idx: usize,
) -> Result<Option<VideoHeader>, MuxError> {
    let start = *gop
        .pic_starts
        .get(pic_idx)
        .ok_or_else(|| invalid("missing picture start"))?;
    let end = sent
        .checked_add(payload_len)
        .ok_or_else(|| invalid("payload overflow"))?;
    if start >= end {
        return Ok(None);
    }

    let global_idx = gop
        .start_pic
        .checked_add(pic_idx)
        .ok_or_else(|| invalid("picture index overflow"))?;
    let pic = video
        .pictures
        .get(global_idx)
        .ok_or_else(|| invalid("missing picture"))?;

    let has_dts = pic.dts != pic.pts;
    let header_len = if has_dts { PTS_DTS_LEN } else { PTS_ONLY_LEN };
    let flags2 = if has_dts {
        PES_FLAGS_PTS_DTS
    } else {
        PES_FLAGS_PTS
    };
    let optional = pack::pts_bytes(to_90k(pic.pts), to_90k(pic.dts), has_dts)?;
    if optional.len() != header_len {
        return Err(invalid("PES header length mismatch"));
    }

    Ok(Some(VideoHeader {
        start,
        header_len,
        flags2,
        optional,
    }))
}

/// First picture start at or after the current video byte.
fn first_pic_start_at_or_after(gop: &GopInfo, state: &GopState) -> Option<usize> {
    if state.next_idx < gop.pic_starts.len()
        && let Some(&start) = gop.pic_starts.get(state.next_idx)
        && start >= state.sent
    {
        return Some(state.next_idx);
    }

    for (idx, &start) in gop.pic_starts.iter().enumerate() {
        if start >= state.sent {
            return Some(idx);
        }
    }
    None
}

/// Build video payload bytes for one pack.
fn build_video_payload(
    gop: &GopInfo,
    video_data: &[u8],
    append_seq_end: bool,
    start: usize,
    len: usize,
) -> Result<Vec<u8>, MuxError> {
    let seq_len = if append_seq_end { SEQ_END.len() } else { 0 };
    let total = gop
        .byte_len
        .checked_add(seq_len)
        .ok_or_else(|| invalid("GOP payload overflow"))?;
    let end = start
        .checked_add(len)
        .ok_or_else(|| invalid("video payload overflow"))?;
    if end > total {
        return Err(invalid("video payload out of range"));
    }

    let mut out = Vec::with_capacity(len);

    if start < gop.byte_len {
        let copy_end = end.min(gop.byte_len);
        let src_start = gop
            .byte_start
            .checked_add(start)
            .ok_or_else(|| invalid("video source overflow"))?;
        let src_end = gop
            .byte_start
            .checked_add(copy_end)
            .ok_or_else(|| invalid("video source overflow"))?;
        let slice = video_data
            .get(src_start..src_end)
            .ok_or_else(|| invalid("video data out of range"))?;
        out.extend_from_slice(slice);
    }

    if end > gop.byte_len {
        if !append_seq_end {
            return Err(invalid("sequence end payload without append"));
        }
        let seq_start = start.max(gop.byte_len);
        let seq_off = seq_start
            .checked_sub(gop.byte_len)
            .ok_or_else(|| invalid("sequence end offset underflow"))?;
        let seq_len = end
            .checked_sub(seq_start)
            .ok_or_else(|| invalid("sequence end length underflow"))?;
        let seq_end = seq_off
            .checked_add(seq_len)
            .ok_or_else(|| invalid("sequence end offset overflow"))?;
        let slice = SEQ_END
            .get(seq_off..seq_end)
            .ok_or_else(|| invalid("sequence end out of range"))?;
        out.extend_from_slice(slice);
    }

    Ok(out)
}

/// Update last pack containing each reference picture.
fn update_ref_last(
    gop: &GopInfo,
    video: &VideoStream,
    payload_start: usize,
    payload_end: usize,
    sector_idx: u32,
    ref_last: &mut [Option<u32>; 3],
) -> Result<(), MuxError> {
    if payload_end <= payload_start {
        return Ok(());
    }

    for (order, maybe_pic) in gop.ref_pics.iter().enumerate() {
        let Some(pic_idx) = *maybe_pic else {
            continue;
        };
        let pic = video
            .pictures
            .get(pic_idx)
            .ok_or_else(|| invalid("missing reference picture"))?;
        let pic_start = pic
            .offset
            .checked_sub(gop.byte_start)
            .ok_or_else(|| invalid("reference picture before GOP"))?;
        let pic_end = pic_start
            .checked_add(pic.len)
            .ok_or_else(|| invalid("reference picture length overflow"))?;

        if pic_start < payload_end
            && payload_start < pic_end
            && let Some(slot) = ref_last.get_mut(order)
        {
            *slot = Some(sector_idx);
        }
    }

    Ok(())
}

/// Advance GOP state after sending bytes up to `payload_end`.
fn advance_state(gop: &GopInfo, state: &mut GopState, payload_end: usize) {
    state.sent = payload_end;
    let len = gop.pic_starts.len();
    if len == 0 {
        state.owner_idx = 0;
        state.next_idx = 0;
        return;
    }

    if payload_end < gop.byte_len {
        let mut owner = 0usize;
        for (i, &start) in gop.pic_starts.iter().enumerate() {
            if start <= payload_end {
                owner = i;
            } else {
                break;
            }
        }
        state.owner_idx = owner;

        let mut next = len;
        for (i, &start) in gop.pic_starts.iter().enumerate() {
            if start >= payload_end {
                next = i;
                break;
            }
        }
        state.next_idx = next;
    } else {
        state.owner_idx = len.saturating_sub(1);
        state.next_idx = len;
    }
}

impl<'a> AudioState<'a> {
    /// Number of remaining frames in this audio stream.
    fn remaining_frames(&self) -> Result<usize, MuxError> {
        match &self.kind {
            AudioKind::Ac3 { stream, .. } => stream
                .frames
                .len()
                .checked_sub(self.next_frame)
                .ok_or_else(|| invalid("AC-3 frame index overflow")),
            AudioKind::Lpcm(lpcm) => match lpcm.data.len().checked_div(lpcm.frame_bytes) {
                Some(total) => total
                    .checked_sub(self.next_frame)
                    .ok_or_else(|| invalid("LPCM frame index overflow")),
                None if lpcm.data.is_empty() => Ok(0),
                None => Err(invalid("LPCM frame_bytes must be non-zero")),
            },
        }
    }

    /// Presentation timestamp of the next frame, in 27 MHz units.
    fn next_pts(&self) -> Result<i64, MuxError> {
        match &self.kind {
            AudioKind::Ac3 { stream, .. } => stream
                .frames
                .get(self.next_frame)
                .map(|frame| frame.pts)
                .ok_or_else(|| invalid("missing AC-3 frame")),
            AudioKind::Lpcm(_) => lpcm_pts(self.start_pts, self.next_frame),
        }
    }

    /// Build one audio PES payload and return its 90 kHz PTS.
    fn build_payload(&mut self, stream_index: usize) -> Result<(Vec<u8>, i64, usize), MuxError> {
        let kind = self.kind;
        let (payload, pts, count) = match kind {
            AudioKind::Ac3 { stream, data } => {
                self.build_ac3_payload(stream, data, stream_index)?
            }
            AudioKind::Lpcm(lpcm) => self.build_lpcm_payload(lpcm, stream_index)?,
        };
        Ok((payload, to_90k(pts), count))
    }

    /// Build one AC-3 PES payload.
    fn build_ac3_payload(
        &mut self,
        stream: &Ac3Stream,
        data: &[u8],
        stream_index: usize,
    ) -> Result<(Vec<u8>, i64, usize), MuxError> {
        let sub_id = sub_stream_id(0x80, stream_index)?;
        let mut count = 0usize;
        let mut total = 0usize;

        while count < 255 {
            let Some(frame_idx) = self.next_frame.checked_add(count) else {
                break;
            };
            let Some(frame) = stream.frames.get(frame_idx) else {
                break;
            };
            let Some(next_total) = total.checked_add(frame.len) else {
                break;
            };
            if next_total > AC3_DATA_CAPACITY {
                break;
            }
            total = next_total;
            count = count.saturating_add(1);
        }

        if count == 0 {
            return Err(invalid("AC-3 frame too large"));
        }

        let first_idx = self.next_frame;
        let first_pts = stream
            .frames
            .get(first_idx)
            .ok_or_else(|| invalid("missing AC-3 frame"))?
            .pts;
        let count_u8 = u8::try_from(count).map_err(|_| invalid("too many AC-3 frames"))?;

        let mut payload = Vec::with_capacity(
            AC3_SUBHEADER_LEN
                .checked_add(total)
                .ok_or_else(|| invalid("AC-3 payload overflow"))?,
        );
        payload.push(sub_id);
        payload.push(count_u8);
        payload.push(0x00);
        payload.push(0x01);

        for k in 0..count {
            let frame_idx = first_idx
                .checked_add(k)
                .ok_or_else(|| invalid("AC-3 frame index overflow"))?;
            let frame = stream
                .frames
                .get(frame_idx)
                .ok_or_else(|| invalid("missing AC-3 frame"))?;
            let end = frame
                .offset
                .checked_add(frame.len)
                .ok_or_else(|| invalid("AC-3 frame length overflow"))?;
            let slice = data
                .get(frame.offset..end)
                .ok_or_else(|| invalid("AC-3 frame data out of range"))?;
            payload.extend_from_slice(slice);
        }

        self.next_frame = first_idx
            .checked_add(count)
            .ok_or_else(|| invalid("AC-3 frame index overflow"))?;
        Ok((payload, first_pts, count))
    }

    /// Build one LPCM PES payload.
    fn build_lpcm_payload(
        &mut self,
        lpcm: &LpcmStream,
        stream_index: usize,
    ) -> Result<(Vec<u8>, i64, usize), MuxError> {
        let sub_id = sub_stream_id(0xA0, stream_index)?;
        if lpcm.frame_bytes == 0 {
            return Err(invalid("LPCM frame_bytes must be non-zero"));
        }

        let total_frames = lpcm.data.len() / lpcm.frame_bytes;
        if self.next_frame >= total_frames {
            return Err(invalid("no LPCM frames"));
        }

        let remaining = total_frames
            .checked_sub(self.next_frame)
            .ok_or_else(|| invalid("LPCM frame index overflow"))?;
        let max_frames = LPCM_DATA_CAPACITY / lpcm.frame_bytes;
        if max_frames == 0 {
            return Err(invalid("LPCM frame too large"));
        }

        let count = remaining.min(max_frames);
        let count_u8 = u8::try_from(count).map_err(|_| invalid("too many LPCM frames"))?;
        let payload_len = count
            .checked_mul(lpcm.frame_bytes)
            .ok_or_else(|| invalid("LPCM payload overflow"))?;
        let byte_start = self
            .next_frame
            .checked_mul(lpcm.frame_bytes)
            .ok_or_else(|| invalid("LPCM offset overflow"))?;
        let byte_end = byte_start
            .checked_add(payload_len)
            .ok_or_else(|| invalid("LPCM payload overflow"))?;
        let samples = lpcm
            .data
            .get(byte_start..byte_end)
            .ok_or_else(|| invalid("LPCM data out of range"))?;

        let frame_no = u8::try_from(self.next_frame % 20)
            .map_err(|_| invalid("LPCM frame number overflow"))?;
        let attr = lpcm
            .channels
            .checked_sub(1)
            .ok_or_else(|| invalid("LPCM channel count must be non-zero"))?;

        let mut payload = Vec::with_capacity(
            LPCM_SUBHEADER_LEN
                .checked_add(payload_len)
                .ok_or_else(|| invalid("LPCM payload overflow"))?,
        );
        payload.push(sub_id);
        payload.push(count_u8);
        payload.push(0x00);
        payload.push(0x01);
        payload.push(frame_no);
        payload.push(attr);
        payload.push(0x80);
        payload.extend_from_slice(samples);

        let pts = lpcm_pts(self.start_pts, self.next_frame)?;
        self.next_frame = self
            .next_frame
            .checked_add(count)
            .ok_or_else(|| invalid("LPCM frame index overflow"))?;
        Ok((payload, pts, count))
    }
}

impl<'a> SubpictureState<'a> {
    /// Whether a unit has been started but not completed.
    fn has_partial(&self) -> bool {
        self.offset > 0
    }

    /// Whether a new unit may be started.
    fn has_pending(&self) -> bool {
        self.offset == 0 && self.next_unit < self.units.len()
    }

    /// Presentation timestamp of the current or next unit, 27 MHz.
    fn next_pts(&self) -> Result<i64, MuxError> {
        self.units
            .get(self.next_unit)
            .map(|unit| unit.pts)
            .ok_or_else(|| invalid("missing subpicture unit"))
    }

    /// Build one subpicture PES payload and return its 90 kHz PTS and first flag.
    fn build_payload(&mut self, stream_index: usize) -> Result<(Vec<u8>, i64, bool), MuxError> {
        let unit = self
            .units
            .get(self.next_unit)
            .ok_or_else(|| invalid("missing subpicture unit"))?;
        let first = self.offset == 0;
        let capacity = if first {
            SUB_FIRST_CAPACITY
        } else {
            SUB_CONT_CAPACITY
        };

        let remaining = unit
            .data
            .len()
            .checked_sub(self.offset)
            .ok_or_else(|| invalid("subpicture offset out of range"))?;
        if remaining == 0 {
            return Err(invalid("empty subpicture unit"));
        }

        let chunk_len = remaining.min(capacity);
        let chunk_end = self
            .offset
            .checked_add(chunk_len)
            .ok_or_else(|| invalid("subpicture chunk overflow"))?;

        let mut payload = Vec::with_capacity(
            SUB_ID_LEN
                .checked_add(chunk_len)
                .ok_or_else(|| invalid("subpicture payload overflow"))?,
        );
        payload.push(sub_stream_id(SUB_STREAM_BASE, stream_index)?);
        let chunk = unit
            .data
            .get(self.offset..chunk_end)
            .ok_or_else(|| invalid("subpicture data out of range"))?;
        payload.extend_from_slice(chunk);

        let pts = unit.pts;
        let unit_len = unit.data.len();
        self.advance(chunk_end, unit_len)?;

        Ok((payload, to_90k(pts), first))
    }

    /// Advance the unit cursor after emitting `new_offset` bytes.
    fn advance(&mut self, new_offset: usize, unit_len: usize) -> Result<(), MuxError> {
        if new_offset >= unit_len {
            self.next_unit = self
                .next_unit
                .checked_add(1)
                .ok_or_else(|| invalid("subpicture unit index overflow"))?;
            self.offset = 0;
        } else {
            self.offset = new_offset;
        }
        Ok(())
    }
}

/// Compute an LPCM frame PTS in 27 MHz units.
fn lpcm_pts(start_pts: i64, frame_index: usize) -> Result<i64, MuxError> {
    let idx = i64::try_from(frame_index).map_err(|_| invalid("too many LPCM frames"))?;
    let add = idx
        .checked_mul(LPCM_FRAME_PTS)
        .ok_or_else(|| invalid("LPCM PTS overflow"))?;
    start_pts
        .checked_add(add)
        .ok_or_else(|| invalid("LPCM PTS overflow"))
}

/// Build a private stream sub stream id.
fn sub_stream_id(base: u8, stream_index: usize) -> Result<u8, MuxError> {
    let idx = u8::try_from(stream_index).map_err(|_| invalid("audio stream index out of range"))?;
    base.checked_add(idx)
        .ok_or_else(|| invalid("audio sub stream overflow"))
}
