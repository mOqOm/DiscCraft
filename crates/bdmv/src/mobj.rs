//! BDMV/MovieObject.bdmv reading and writing.

use crate::{BdmvError, Reader, VERSION, len_u32};

const HEADER_RESERVED_LEN: usize = 28;
const COMMAND_LEN: usize = 12;
const OBJECT_FIXED_LEN: usize = 4;
const MOVIE_OBJECTS_FIXED_LEN: usize = 6;

/// One 12-byte HDMV navigation command (opcode word, destination operand, source operand).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavCommand(#[doc = "Raw big-endian command bytes."] pub [u8; 12]);

impl NavCommand {
    /// Build a command from raw big-endian fields.
    pub fn new(opcode: u32, dst: u32, src: u32) -> Self {
        let mut buf = [0u8; 12];
        let (op, rest) = buf.split_at_mut(4);
        op.copy_from_slice(&opcode.to_be_bytes());
        let (d, s) = rest.split_at_mut(4);
        d.copy_from_slice(&dst.to_be_bytes());
        s.copy_from_slice(&src.to_be_bytes());
        Self(buf)
    }

    /// Return the 32-bit opcode word.
    pub fn opcode(&self) -> u32 {
        let (op, _) = self.0.split_at(4);
        be_u32(op)
    }

    /// Return the destination operand.
    pub fn dst(&self) -> u32 {
        let (_, rest) = self.0.split_at(4);
        let (d, _) = rest.split_at(4);
        be_u32(d)
    }

    /// Return the source operand.
    pub fn src(&self) -> u32 {
        let (_, rest) = self.0.split_at(4);
        let (_, s) = rest.split_at(4);
        be_u32(s)
    }

    /// Branch_Nop: group 0, sub-group 0, branch option 0, no operands.
    pub fn nop() -> Self {
        let op = opcode(0, 0, 0, false, false, 0, OptionKind::Branch);
        Self::new(op, 0, 0)
    }

    /// Branch_PlayList: group 0, sub-group 2, branch option 0, one immediate destination.
    pub fn play_pl(playlist: u32) -> Self {
        let op = opcode(1, 0, 2, true, false, 0, OptionKind::Branch);
        Self::new(op, playlist, 0)
    }

    /// Branch_PlayMark: group 0, sub-group 2, branch option 2, two immediate operands.
    pub fn play_pl_mark(playlist: u32, mark: u32) -> Self {
        let op = opcode(2, 0, 2, true, true, 2, OptionKind::Branch);
        Self::new(op, playlist, mark)
    }

    /// Branch_JumpTitle: group 0, sub-group 1, branch option 1, immediate destination.
    pub fn jump_title(title: u32) -> Self {
        let op = opcode(1, 0, 1, true, false, 1, OptionKind::Branch);
        Self::new(op, title, 0)
    }

    /// Branch_JumpObject: group 0, sub-group 1, branch option 0, immediate destination.
    pub fn jump_object(object: u32) -> Self {
        let op = opcode(1, 0, 1, true, false, 0, OptionKind::Branch);
        Self::new(op, object, 0)
    }

    /// Branch_CallTitle: group 0, sub-group 1, branch option 3, immediate destination.
    pub fn call_title(title: u32) -> Self {
        let op = opcode(1, 0, 1, true, false, 3, OptionKind::Branch);
        Self::new(op, title, 0)
    }

    /// Set_Move: group 2 (Set), sub-group 0, set option 1; destination is a GPR index, source is immediate.
    pub fn set_reg(gpr: u32, value: u32) -> Self {
        let op = opcode(2, SET_GROUP, 0, false, true, 1, OptionKind::Set);
        Self::new(op, gpr, value)
    }

    /// Set_Sub: group 2 (Set), sub-group 0, set option 4; subtracts an immediate value from a GPR.
    pub fn sub_reg(gpr: u32, value: u32) -> Self {
        let op = opcode(2, SET_GROUP, 0, false, true, 4, OptionKind::Set);
        Self::new(op, gpr, value)
    }

    /// Set_ButtonPage: group 2 (Set), sub-group 1, set option 3. Selects `button` on `page` of the next interactive
    /// composition (the button and page flags in the top bits say both are set).
    pub fn set_button_page(button: u16, page: u16) -> Self {
        let op = opcode(2, SET_GROUP, 1, true, true, 3, OptionKind::Set);
        Self::new(
            op,
            BUTTON_PAGE_FLAG | u32::from(button),
            BUTTON_PAGE_FLAG | u32::from(page),
        )
    }

    /// Set_SetStream: group 2 (Set), sub-group 1, set option 1. Selects the primary audio stream (1-based) and the
    /// PG stream (1-based, with whether it is shown); `None` leaves that one alone. The flags in the destination's
    /// bits 31 (audio) and 15 (PG) say which are set; bit 14 shows the PG stream.
    pub fn set_stream(audio: Option<u16>, pg: Option<(bool, u16)>) -> Self {
        let mut dst = 0_u32;
        if let Some(audio) = audio {
            dst |= BUTTON_PAGE_FLAG | (u32::from(audio & 0x0FFF) << 16);
        }
        if let Some((shown, stream)) = pg {
            dst |= 0x8000 | u32::from(stream & 0x0FFF);
            if shown {
                dst |= 0x4000;
            }
        }
        let op = opcode(2, SET_GROUP, 1, true, true, 1, OptionKind::Set);
        Self::new(op, dst, 0)
    }

    /// Branch_GoTo: group 0, sub-group 0, branch option 1, immediate destination.
    pub fn goto_line(line: u32) -> Self {
        let op = opcode(1, 0, 0, true, false, 1, OptionKind::Branch);
        Self::new(op, line, 0)
    }

    /// Compare_EQ: group 1, compare option 2; destination is a GPR index, source is immediate.
    pub fn if_eq(gpr: u32, value: u32) -> Self {
        let op = opcode(2, 1, 0, false, true, 2, OptionKind::Compare);
        Self::new(op, gpr, value)
    }
}

/// One movie object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovieObject {
    /// ResumeIntentionFlag.
    pub resume_intention: bool,
    /// MenuCallMask.
    pub menu_call_mask: bool,
    /// TitleSearchMask.
    pub title_search_mask: bool,
    /// Navigation commands.
    pub commands: Vec<NavCommand>,
}

/// The command group of the Set commands: 10b. (The format notes we started from listed 11b; libbluray, the
/// black-box oracle, reads 11b as an unknown group and skips the command, and takes 10b as Set.)
const SET_GROUP: u8 = 2;

/// Set_ButtonPage's button / page flag (the top bit of each operand).
const BUTTON_PAGE_FLAG: u32 = 0x8000_0000;

#[derive(Clone, Copy)]
enum OptionKind {
    Branch,
    Compare,
    Set,
}

fn be_u32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0u32, |acc, b| (acc << 8) | u32::from(*b))
}

fn opcode(
    operand_count: u8,
    group: u8,
    subgroup: u8,
    imm_dst: bool,
    imm_src: bool,
    option: u8,
    kind: OptionKind,
) -> u32 {
    let mut word = 0u32;
    word |= u32::from(operand_count & 0b111) << 29;
    word |= u32::from(group & 0b11) << 27;
    word |= u32::from(subgroup & 0b111) << 24;

    if imm_dst {
        word |= 1u32 << 23;
    }
    if imm_src {
        word |= 1u32 << 22;
    }

    match kind {
        OptionKind::Branch => word |= u32::from(option & 0b1111) << 16,
        OptionKind::Compare => word |= u32::from(option & 0b1111) << 8,
        OptionKind::Set => word |= u32::from(option & 0b11111),
    }

    word
}

fn movie_objects_len(objects: &[MovieObject]) -> Result<u32, BdmvError> {
    let mut content = MOVIE_OBJECTS_FIXED_LEN;

    for object in objects {
        let command_bytes = object
            .commands
            .len()
            .checked_mul(COMMAND_LEN)
            .ok_or_else(|| BdmvError("movie object length overflow".to_string()))?;
        let object_bytes = command_bytes
            .checked_add(OBJECT_FIXED_LEN)
            .ok_or_else(|| BdmvError("movie object length overflow".to_string()))?;
        content = content
            .checked_add(object_bytes)
            .ok_or_else(|| BdmvError("movie objects length overflow".to_string()))?;
    }

    len_u32(content)
}

fn write_object(out: &mut Vec<u8>, object: &MovieObject) -> Result<(), BdmvError> {
    let count = u16::try_from(object.commands.len())
        .map_err(|_| BdmvError("too many navigation commands".to_string()))?;

    let mut flags = 0u8;
    if object.resume_intention {
        flags |= 0x80;
    }
    if object.menu_call_mask {
        flags |= 0x40;
    }
    if object.title_search_mask {
        flags |= 0x20;
    }

    out.push(flags);
    out.push(0);
    out.extend_from_slice(&count.to_be_bytes());

    for command in &object.commands {
        out.extend_from_slice(&command.0);
    }

    Ok(())
}

/// Write a `MovieObject.bdmv` file.
pub fn write_movie_objects(objects: &[MovieObject]) -> Result<Vec<u8>, BdmvError> {
    let length = movie_objects_len(objects)?;
    let count = u16::try_from(objects.len())
        .map_err(|_| BdmvError("too many movie objects".to_string()))?;

    let mut out = Vec::new();
    out.extend_from_slice(b"MOBJ");
    out.extend_from_slice(VERSION);
    out.extend_from_slice(&0u32.to_be_bytes()); // ExtensionDataStartAddress.
    out.extend_from_slice(&[0u8; HEADER_RESERVED_LEN]);

    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes()); // reserved.
    out.extend_from_slice(&count.to_be_bytes());

    for object in objects {
        write_object(&mut out, object)?;
    }

    Ok(out)
}

fn read_header(r: &mut Reader<'_>) -> Result<(), BdmvError> {
    if !r.bytes(4)?.starts_with(b"MOBJ") {
        return Err(BdmvError("bad movie object type indicator".to_string()));
    }
    if !r.bytes(4)?.starts_with(VERSION) {
        return Err(BdmvError("bad movie object version".to_string()));
    }
    if r.u32()? != 0 {
        return Err(BdmvError(
            "movie object extension data is not supported".to_string(),
        ));
    }
    r.skip(HEADER_RESERVED_LEN)
}

fn read_object(r: &mut Reader<'_>) -> Result<MovieObject, BdmvError> {
    let flags = r.bytes(2)?;
    let first = flags
        .first()
        .copied()
        .ok_or_else(|| BdmvError("missing object flags".to_string()))?;

    let count = usize::from(r.u16()?);
    let mut commands = Vec::with_capacity(count);

    for _ in 0..count {
        let bytes = r.bytes(COMMAND_LEN)?;
        let mut raw = [0u8; COMMAND_LEN];
        raw.copy_from_slice(bytes);
        commands.push(NavCommand(raw));
    }

    Ok(MovieObject {
        resume_intention: first & 0x80 != 0,
        menu_call_mask: first & 0x40 != 0,
        title_search_mask: first & 0x20 != 0,
        commands,
    })
}

fn read_objects(r: &mut Reader<'_>, count: usize) -> Result<Vec<MovieObject>, BdmvError> {
    let mut objects = Vec::with_capacity(count);
    for _ in 0..count {
        objects.push(read_object(r)?);
    }
    Ok(objects)
}

/// Read a `MovieObject.bdmv` file.
pub fn read_movie_objects(data: &[u8]) -> Result<Vec<MovieObject>, BdmvError> {
    let mut r = Reader::at(data, 0);
    read_header(&mut r)?;

    let length_pos = r.pos();
    let length = usize::try_from(r.u32()?)
        .map_err(|_| BdmvError("movie objects length out of range".to_string()))?;
    let expected_end = length_pos
        .checked_add(4)
        .and_then(|x| x.checked_add(length))
        .ok_or_else(|| BdmvError("movie objects length overflow".to_string()))?;

    r.skip(4)?;
    let count = usize::from(r.u16()?);
    let objects = read_objects(&mut r, count)?;

    if r.pos() != expected_end {
        return Err(BdmvError("movie objects length mismatch".to_string()));
    }

    Ok(objects)
}

#[cfg(test)]
mod tests {
    use super::{MovieObject, NavCommand, read_movie_objects, write_movie_objects};

    fn sample() -> Vec<MovieObject> {
        vec![
            MovieObject {
                resume_intention: true,
                menu_call_mask: false,
                title_search_mask: true,
                commands: vec![
                    NavCommand::nop(),
                    NavCommand::play_pl(3),
                    NavCommand::jump_title(2),
                ],
            },
            MovieObject {
                resume_intention: false,
                menu_call_mask: true,
                title_search_mask: false,
                commands: vec![
                    NavCommand::set_reg(1, 5),
                    NavCommand::if_eq(1, 5),
                    NavCommand::goto_line(7),
                ],
            },
        ]
    }

    #[test]
    fn round_trip() -> Result<(), crate::BdmvError> {
        let data = write_movie_objects(&sample())?;
        let back = read_movie_objects(&data)?;
        assert_eq!(back, sample());
        Ok(())
    }

    #[test]
    fn header() -> Result<(), crate::BdmvError> {
        let data = write_movie_objects(&sample())?;
        assert!(data.starts_with(b"MOBJ0200"));
        Ok(())
    }

    #[test]
    fn command_examples() {
        assert_eq!(
            NavCommand::play_pl(0).0,
            [0x22, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            NavCommand::jump_title(1).0,
            [0x21, 0x81, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]
        );
    }

    #[test]
    fn set_and_compare_fields() {
        let set = NavCommand::set_reg(1, 5);
        let op = set.opcode();
        assert_eq!((op >> 29) & 7, 2);
        assert_eq!((op >> 27) & 3, 2);
        assert_eq!((op >> 24) & 7, 0);
        assert_eq!((op >> 23) & 1, 0);
        assert_eq!((op >> 22) & 1, 1);
        assert_eq!(op & 0x1F, 1);
        assert_eq!(set.dst(), 1);
        assert_eq!(set.src(), 5);

        let cmp = NavCommand::if_eq(1, 5);
        let op = cmp.opcode();
        assert_eq!((op >> 29) & 7, 2);
        assert_eq!((op >> 27) & 3, 1);
        assert_eq!((op >> 24) & 7, 0);
        assert_eq!((op >> 23) & 1, 0);
        assert_eq!((op >> 22) & 1, 1);
        assert_eq!((op >> 8) & 0xF, 2);
        assert_eq!(cmp.dst(), 1);
        assert_eq!(cmp.src(), 5);

        let stream = NavCommand::set_stream(Some(2), Some((false, 1)));
        let op = stream.opcode();
        assert_eq!(((op >> 27) & 3, (op >> 24) & 7, op & 0x1F), (2, 1, 1));
        assert_eq!((stream.dst(), stream.src()), (0x8002_8001, 0));
        assert_eq!(NavCommand::set_stream(None, Some((true, 3))).dst(), 0xC003);

        let page = NavCommand::set_button_page(2, 0);
        let op = page.opcode();
        assert_eq!(((op >> 27) & 3, (op >> 24) & 7, op & 0x1F), (2, 1, 3));
        assert_eq!((page.dst(), page.src()), (0x8000_0002, 0x8000_0000));

        // Sub differs from Move only in the set option.
        let sub = NavCommand::sub_reg(1, 1);
        assert_eq!(sub.opcode() & !0x1F, set.opcode() & !0x1F);
        assert_eq!(sub.opcode() & 0x1F, 4);
        assert_eq!((sub.dst(), sub.src()), (1, 1));
    }

    #[test]
    fn truncated() -> Result<(), crate::BdmvError> {
        let mut data = write_movie_objects(&sample())?;
        let _ = data.pop();
        assert!(read_movie_objects(&data).is_err());
        Ok(())
    }
}
