//! The Blu-ray (HDMV) navigation of a built `BDMV` folder, run for the preview: the index table, the movie
//! objects, each playlist's chapters and the interactive graphics (menus and pop-up menus) of its clip, and a
//! small HDMV interpreter over them.
//!
//! The interpreter runs the commands the Blu-ray build writes (BD-ROM Part 3, the HDMV navigation commands): the
//! branches (GoTo, JumpObject, JumpTitle, PlayPL, PlayPL at a mark), the compares, the GPR Set commands
//! (Move, Add, Sub) and the SetSystem commands SetStream and SetButtonPage. Others are an error. A movie object
//! that plays a playlist waits there; when the playlist ends ([`BdVm::finish`]) the object goes on from the next
//! command. A menu is the interactive composition of the playing playlist's clip: the arrows move between the
//! buttons by their neighbours, Enter runs the button's commands. A pop-up composition is shown with the pop-up
//! key over the playing title.

use std::collections::BTreeMap;
use std::path::Path;

use disccraft_bdmv::index::read_index;
use disccraft_bdmv::mobj::{MovieObject, read_movie_objects};
use disccraft_bdmv::mpls::read_mpls;
use disccraft_pgs::ig::IgPage;

use crate::{Key, VmError};

/// How much of a clip the loader reads for its interactive graphics: a menu clip is short, and a title's pop-up
/// menu is multiplexed at its start.
const IG_SCAN_BYTES: u64 = 16 * 1024 * 1024;
/// How many commands one key press may run before the navigation counts as looping.
const MAX_STEPS: usize = 100_000;
/// The GPRs an HDMV player has.
const GPR_COUNT: u32 = 4096;
/// No button.
const NO_BUTTON: u16 = 0xFFFF;
/// The register operand's PSR flag.
const PSR_FLAG: u32 = 0x8000_0000;

/// A built Blu-ray disc's navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BdDisc {
    /// The movie object run when the disc starts.
    pub first_playback: u16,
    /// The movie object of the Top Menu key.
    pub top_menu: u16,
    /// Each title's movie object (title 1 first).
    pub titles: Vec<u16>,
    /// The movie objects.
    pub objects: Vec<MovieObject>,
    /// The playlists by number.
    pub playlists: BTreeMap<u32, BdPlaylist>,
}

/// One playlist of the disc.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BdPlaylist {
    /// How many marks (chapters) it has.
    pub marks: u32,
    /// The interactive graphics of its (first) clip: a menu, or a pop-up menu over a title.
    pub menu: Option<BdMenu>,
}

/// A playlist's interactive graphics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BdMenu {
    /// Shown with the pop-up key over the playing playlist, rather than always.
    pub popup: bool,
    /// The pages.
    pub pages: Vec<IgPage>,
}

/// Where the disc is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BdLocation {
    /// Nothing plays.
    #[default]
    Stopped,
    /// A playlist plays from `mark` (0-based).
    Playing {
        /// The playlist number.
        playlist: u32,
        /// The mark it plays from.
        mark: u32,
    },
}

/// The menu on screen: the playing playlist's interactive graphics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BdMenuView {
    /// The page index.
    pub page: u8,
    /// The selected button's id.
    pub button: u16,
    /// For a pop-up menu, whether it is shown.
    pub open: bool,
}

/// What the disc keeps between key presses.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BdState {
    /// Where it is.
    pub location: BdLocation,
    /// The movie object waiting for the playing playlist to end, and the command to go on from.
    pub resume: Option<(u16, usize)>,
    /// The menu on screen.
    pub menu: Option<BdMenuView>,
    /// The general purpose registers that are not 0.
    pub gpr: BTreeMap<u32, u32>,
    /// PSR1: the primary audio stream (1-based).
    pub audio: u16,
    /// PSR2: the PG stream (1-based) and whether it is shown.
    pub subtitle: (bool, u16),
    /// PSR10 / PSR11: the button and page Set_ButtonPage asked the next menu for.
    pub button_page: (Option<u16>, Option<u16>),
}

/// Load the navigation of a built disc: `dir` is the `BDMV` folder or the folder that holds it.
///
/// # Errors
///
/// A missing or unreadable `index.bdmv` or `MovieObject.bdmv`, or a playlist or clip that cannot be read.
pub fn load_bdmv(dir: &Path) -> Result<BdDisc, VmError> {
    let bdmv = if dir.join("index.bdmv").is_file() {
        dir.to_path_buf()
    } else {
        dir.join("BDMV")
    };
    let index = read_index(&std::fs::read(bdmv.join("index.bdmv"))?)
        .map_err(|error| VmError::Invalid(format!("index.bdmv: {}", error.0)))?;
    let objects = read_movie_objects(&std::fs::read(bdmv.join("MovieObject.bdmv"))?)
        .map_err(|error| VmError::Invalid(format!("MovieObject.bdmv: {}", error.0)))?;
    let mut playlists = BTreeMap::new();
    for entry in std::fs::read_dir(bdmv.join("PLAYLIST"))? {
        let path = entry?.path();
        let Some(number) = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<u32>().ok())
            .filter(|_| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("mpls"))
            })
        else {
            continue;
        };
        let playlist = read_mpls(&std::fs::read(&path)?)
            .map_err(|error| VmError::Invalid(format!("{}: {}", path.display(), error.0)))?;
        let marks = u32::try_from(playlist.marks.len()).unwrap_or(u32::MAX);
        let menu = match playlist.items.first() {
            Some(item) if !item.streams.ig.is_empty() => {
                let clip = String::from_utf8_lossy(&item.clip).into_owned();
                clip_menu(&bdmv.join("STREAM").join(format!("{clip}.m2ts")))?
            }
            _ => None,
        };
        let _ = playlists.insert(number, BdPlaylist { marks, menu });
    }
    Ok(BdDisc {
        first_playback: index.first_playback,
        top_menu: index.top_menu,
        titles: index
            .titles
            .iter()
            .map(|title| title.movie_object)
            .collect(),
        objects,
        playlists,
    })
}

/// The first interactive composition of a clip's IG stream, read from its start.
fn clip_menu(path: &Path) -> Result<Option<BdMenu>, VmError> {
    use std::io::Read;
    let mut head = Vec::new();
    let _ = std::fs::File::open(path)?
        .take(IG_SCAN_BYTES)
        .read_to_end(&mut head)?;
    let payloads = disccraft_m2tsmux::pes_payloads(&head, disccraft_m2tsmux::IG_PID)
        .map_err(|error| VmError::Invalid(format!("{}: {}", path.display(), error.0)))?;
    let compositions = disccraft_pgs::read::interactive_compositions(&payloads.concat())
        .map_err(|error| VmError::Invalid(format!("{}: {}", path.display(), error.0)))?;
    Ok(compositions.into_iter().next().map(|composition| BdMenu {
        popup: composition.popup,
        pages: composition.pages,
    }))
}

/// One decoded HDMV command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Nop,
    GoTo(u32),
    JumpObject(u32),
    JumpTitle(u32),
    PlayPl(u32, u32),
    Compare(Compare, Operand, Operand),
    Set(SetOp, u32, Operand),
    SetStream(u32),
    SetButtonPage(u32, u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Compare {
    Eq,
    Ne,
    Ge,
    Gt,
    Le,
    Lt,
    BitAnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetOp {
    Move,
    Add,
    Sub,
}

/// An operand: an immediate value, or a register (GPR, or PSR with its flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operand {
    Immediate(u32),
    Register(u32),
}

fn decode(command: &disccraft_bdmv::mobj::NavCommand) -> Result<Command, VmError> {
    let op = command.opcode();
    let (dst, src) = (command.dst(), command.src());
    let group = (op >> 27) & 0b11;
    let subgroup = (op >> 24) & 0b111;
    let immediate_dst = op & (1 << 23) != 0;
    let immediate_src = op & (1 << 22) != 0;
    let operand = |value: u32, immediate: bool| {
        if immediate {
            Operand::Immediate(value)
        } else {
            Operand::Register(value)
        }
    };
    let unsupported = || VmError::Unsupported(format!("HDMV command {op:08x} {dst:08x} {src:08x}"));
    match (group, subgroup) {
        (0, 0) => match (op >> 16) & 0xF {
            0 => Ok(Command::Nop),
            1 => Ok(Command::GoTo(dst)),
            _ => Err(unsupported()),
        },
        (0, 1) => match (op >> 16) & 0xF {
            0 => Ok(Command::JumpObject(dst)),
            1 => Ok(Command::JumpTitle(dst)),
            _ => Err(unsupported()),
        },
        (0, 2) => match (op >> 16) & 0xF {
            0 => Ok(Command::PlayPl(dst, 0)),
            2 => Ok(Command::PlayPl(dst, src)),
            _ => Err(unsupported()),
        },
        (1, _) => {
            let compare = match (op >> 8) & 0xF {
                1 => Compare::BitAnd,
                2 => Compare::Eq,
                3 => Compare::Ne,
                4 => Compare::Ge,
                5 => Compare::Gt,
                6 => Compare::Le,
                7 => Compare::Lt,
                _ => return Err(unsupported()),
            };
            Ok(Command::Compare(
                compare,
                operand(dst, immediate_dst),
                operand(src, immediate_src),
            ))
        }
        (2, 0) => {
            let set = match op & 0x1F {
                1 => SetOp::Move,
                3 => SetOp::Add,
                4 => SetOp::Sub,
                _ => return Err(unsupported()),
            };
            if immediate_dst {
                return Err(unsupported());
            }
            Ok(Command::Set(set, dst, operand(src, immediate_src)))
        }
        (2, 1) => match op & 0x1F {
            1 => Ok(Command::SetStream(dst)),
            3 => Ok(Command::SetButtonPage(dst, src)),
            _ => Err(unsupported()),
        },
        _ => Err(unsupported()),
    }
}

/// The HDMV interpreter over a [`BdDisc`].
#[derive(Debug)]
pub struct BdVm<'a> {
    disc: &'a BdDisc,
    state: BdState,
}

/// What running commands came to.
enum Ran {
    /// A playlist plays (the commands wait for it).
    Playing,
    /// The commands ran out (a movie object ends; a button's commands go back to its menu).
    Ended,
}

impl<'a> BdVm<'a> {
    /// Start the disc: its first playback object.
    ///
    /// # Errors
    ///
    /// Navigation that cannot run (see [`BdVm::press`]).
    pub fn start(disc: &'a BdDisc) -> Result<Self, VmError> {
        let mut vm = Self {
            disc,
            state: BdState::default(),
        };
        vm.run_object(disc.first_playback, 0)?;
        Ok(vm)
    }

    /// Go on from `state` (a preview between key presses).
    pub fn restore(disc: &'a BdDisc, state: BdState) -> Self {
        Self { disc, state }
    }

    /// Where the disc is and what it keeps.
    pub fn state(&self) -> &BdState {
        &self.state
    }

    /// The menu on screen and its buttons: the playing playlist's interactive graphics when they are shown (an
    /// always-on menu, or an open pop-up).
    pub fn menu(&self) -> Option<(&'a BdMenu, BdMenuView)> {
        let view = self.state.menu?;
        let menu = self.playing_menu()?;
        (!menu.popup || view.open).then_some((menu, view))
    }

    /// Whether the playing playlist has a pop-up menu.
    pub fn has_popup(&self) -> bool {
        self.playing_menu().is_some_and(|menu| menu.popup)
    }

    fn playing_menu(&self) -> Option<&'a BdMenu> {
        let BdLocation::Playing { playlist, .. } = self.state.location else {
            return None;
        };
        self.disc.playlists.get(&playlist)?.menu.as_ref()
    }

    /// A menu key: the arrows move to the selected button's neighbour (an auto-action button then runs), Enter
    /// runs the selected button's commands. Without a menu on screen nothing happens.
    ///
    /// # Errors
    ///
    /// Commands outside the supported subset, a missing object, title or playlist, or navigation that loops.
    pub fn press(&mut self, key: Key) -> Result<(), VmError> {
        let Some((menu, view)) = self.menu() else {
            return Ok(());
        };
        let page = menu
            .pages
            .get(usize::from(view.page))
            .ok_or_else(|| VmError::Invalid("menu page does not exist".to_string()))?;
        let button = page
            .buttons
            .iter()
            .find(|button| button.id == view.button)
            .ok_or_else(|| VmError::Invalid("selected button does not exist".to_string()))?;
        let target = match key {
            Key::Enter => return self.activate(view.page, view.button),
            Key::Up => button.upper,
            Key::Down => button.lower,
            Key::Left => button.left,
            Key::Right => button.right,
        };
        let Some(next) = page.buttons.iter().find(|button| button.id == target) else {
            return Ok(());
        };
        if let Some(view) = self.state.menu.as_mut() {
            view.button = next.id;
        }
        if next.auto_action {
            return self.activate(view.page, next.id);
        }
        Ok(())
    }

    /// Run button `id`'s commands on `page`. When they end without a branch, the menu stays.
    fn activate(&mut self, page: u8, id: u16) -> Result<(), VmError> {
        let Some((menu, _)) = self.menu() else {
            return Ok(());
        };
        let commands = menu
            .pages
            .get(usize::from(page))
            .and_then(|page| page.buttons.iter().find(|button| button.id == id))
            .map(|button| {
                button
                    .commands
                    .iter()
                    .map(|bytes| disccraft_bdmv::mobj::NavCommand(*bytes))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let popup = menu.popup;
        let before = self.state.location;
        if let Ran::Ended = self.run(&commands, None)? {
            return Ok(());
        }
        // A pop-up closes when its button takes the disc elsewhere.
        if popup
            && self.state.location != before
            && let Some(view) = self.state.menu.as_mut()
        {
            view.open = false;
        }
        Ok(())
    }

    /// The playing playlist ends: the movie object that played it goes on.
    ///
    /// # Errors
    ///
    /// As [`BdVm::press`].
    pub fn finish(&mut self) -> Result<(), VmError> {
        match self.state.resume.take() {
            Some((object, line)) => self.run_object(object, line),
            None => {
                self.stop();
                Ok(())
            }
        }
    }

    /// The Top Menu key: the disc's top menu object.
    ///
    /// # Errors
    ///
    /// As [`BdVm::press`].
    pub fn top_menu(&mut self) -> Result<(), VmError> {
        self.run_object(self.disc.top_menu, 0)
    }

    /// The pop-up menu key: shows or hides the playing playlist's pop-up menu. Without one nothing happens.
    pub fn popup_key(&mut self) {
        if !self.has_popup() {
            return;
        }
        if let Some(view) = self.state.menu.as_mut() {
            view.open = !view.open;
        }
    }

    /// The chapter keys and number: play the playing playlist from `mark` (0-based; past the last mark nothing
    /// happens).
    ///
    /// # Errors
    ///
    /// When nothing plays.
    pub fn jump_mark(&mut self, mark: u32) -> Result<(), VmError> {
        let BdLocation::Playing { playlist, .. } = self.state.location else {
            return Err(VmError::Invalid("nothing plays".to_string()));
        };
        let marks = self
            .disc
            .playlists
            .get(&playlist)
            .map_or(0, |playlist| playlist.marks);
        if mark < marks.max(1) {
            self.state.location = BdLocation::Playing { playlist, mark };
        }
        Ok(())
    }

    fn stop(&mut self) {
        self.state.location = BdLocation::Stopped;
        self.state.menu = None;
        self.state.resume = None;
    }

    /// Run movie object `object` from command `line`.
    fn run_object(&mut self, object: u16, line: usize) -> Result<(), VmError> {
        let commands = self
            .disc
            .objects
            .get(usize::from(object))
            .ok_or_else(|| VmError::Invalid(format!("movie object {object} does not exist")))?
            .commands
            .clone();
        match self.run_from(&commands, Some(object), line)? {
            Ran::Playing => Ok(()),
            Ran::Ended => {
                self.stop();
                Ok(())
            }
        }
    }

    fn run(
        &mut self,
        commands: &[disccraft_bdmv::mobj::NavCommand],
        object: Option<u16>,
    ) -> Result<Ran, VmError> {
        self.run_from(commands, object, 0)
    }

    /// Run `commands` (movie object `object`'s, or a button's when `None`) from `line`.
    fn run_from(
        &mut self,
        commands: &[disccraft_bdmv::mobj::NavCommand],
        object: Option<u16>,
        line: usize,
    ) -> Result<Ran, VmError> {
        let mut commands = commands.to_vec();
        let mut object = object;
        let mut pc = line;
        for _ in 0..MAX_STEPS {
            let Some(command) = commands.get(pc) else {
                return Ok(Ran::Ended);
            };
            pc = pc.saturating_add(1);
            match decode(command)? {
                Command::Nop => {}
                Command::GoTo(target) => {
                    pc = usize::try_from(target).unwrap_or(usize::MAX);
                }
                Command::JumpObject(target) => {
                    let target = u16::try_from(target)
                        .map_err(|_| VmError::Invalid("movie object out of range".to_string()))?;
                    commands = self.object_commands(target)?;
                    object = Some(target);
                    pc = 0;
                }
                Command::JumpTitle(title) => {
                    let target = usize::try_from(title)
                        .ok()
                        .and_then(|title| title.checked_sub(1))
                        .and_then(|index| self.disc.titles.get(index))
                        .copied()
                        .ok_or_else(|| VmError::Invalid(format!("title {title} does not exist")))?;
                    commands = self.object_commands(target)?;
                    object = Some(target);
                    pc = 0;
                }
                Command::PlayPl(playlist, mark) => {
                    self.play(playlist, mark)?;
                    // A movie object waits for the playlist; a button's play leaves the waiting object as it was.
                    if let Some(object) = object {
                        self.state.resume = Some((object, pc));
                    }
                    return Ok(Ran::Playing);
                }
                Command::Compare(compare, left, right) => {
                    let (left, right) = (self.value(left), self.value(right));
                    let holds = match compare {
                        Compare::Eq => left == right,
                        Compare::Ne => left != right,
                        Compare::Ge => left >= right,
                        Compare::Gt => left > right,
                        Compare::Le => left <= right,
                        Compare::Lt => left < right,
                        Compare::BitAnd => left & right != 0,
                    };
                    // A compare that does not hold skips the next command.
                    if !holds {
                        pc = pc.saturating_add(1);
                    }
                }
                Command::Set(set, register, source) => {
                    let value = self.value(source);
                    let current = self.value(Operand::Register(register));
                    let result = match set {
                        SetOp::Move => value,
                        SetOp::Add => current.wrapping_add(value),
                        SetOp::Sub => current.saturating_sub(value),
                    };
                    self.set_register(register, result)?;
                }
                Command::SetStream(streams) => {
                    if streams & 0x8000_0000 != 0 {
                        self.state.audio = u16::try_from((streams >> 16) & 0x0FFF).unwrap_or(0);
                    }
                    if streams & 0x8000 != 0 {
                        self.state.subtitle = (
                            streams & 0x4000 != 0,
                            u16::try_from(streams & 0x0FFF).unwrap_or(0),
                        );
                    }
                }
                Command::SetButtonPage(button, page) => {
                    if button & 0x8000_0000 != 0 {
                        self.state.button_page.0 =
                            Some(u16::try_from(button & 0xFFFF).unwrap_or(NO_BUTTON));
                    }
                    if page & 0x8000_0000 != 0 {
                        self.state.button_page.1 = Some(u16::try_from(page & 0xFFFF).unwrap_or(0));
                    }
                }
            }
        }
        Err(VmError::Invalid("the navigation loops".to_string()))
    }

    fn object_commands(
        &self,
        object: u16,
    ) -> Result<Vec<disccraft_bdmv::mobj::NavCommand>, VmError> {
        self.disc
            .objects
            .get(usize::from(object))
            .map(|found| found.commands.clone())
            .ok_or_else(|| VmError::Invalid(format!("movie object {object} does not exist")))
    }

    /// Play `playlist` from `mark`: its menu comes up with the button (and page) Set_ButtonPage asked for, or the
    /// page's default button, or its first.
    fn play(&mut self, playlist: u32, mark: u32) -> Result<(), VmError> {
        let found = self
            .disc
            .playlists
            .get(&playlist)
            .ok_or_else(|| VmError::Invalid(format!("playlist {playlist} does not exist")))?;
        self.state.location = BdLocation::Playing { playlist, mark };
        let (button, page) = std::mem::take(&mut self.state.button_page);
        self.state.menu = found.menu.as_ref().and_then(|menu| {
            let index = page
                .filter(|page| usize::from(*page) < menu.pages.len())
                .unwrap_or(0);
            let shown = menu.pages.get(usize::from(index))?;
            let has = |id: u16| shown.buttons.iter().any(|button| button.id == id);
            let button = button
                .filter(|id| has(*id))
                .or(Some(shown.default_selected).filter(|id| has(*id)))
                .or_else(|| shown.buttons.first().map(|button| button.id))?;
            Some(BdMenuView {
                page: u8::try_from(index).unwrap_or(0),
                button,
                open: false,
            })
        });
        Ok(())
    }

    fn value(&self, operand: Operand) -> u32 {
        match operand {
            Operand::Immediate(value) => value,
            Operand::Register(register) if register & PSR_FLAG != 0 => match register & 0x7F {
                1 => u32::from(self.state.audio),
                2 => {
                    u32::from(self.state.subtitle.1)
                        | if self.state.subtitle.0 {
                            0x8000_0000
                        } else {
                            0
                        }
                }
                10 => u32::from(self.state.menu.map_or(NO_BUTTON, |view| view.button)),
                _ => 0,
            },
            Operand::Register(register) => self.state.gpr.get(&register).copied().unwrap_or(0),
        }
    }

    fn set_register(&mut self, register: u32, value: u32) -> Result<(), VmError> {
        if register & PSR_FLAG != 0 || register >= GPR_COUNT {
            return Err(VmError::Unsupported(format!("set register {register:#x}")));
        }
        if value == 0 {
            let _ = self.state.gpr.remove(&register);
        } else {
            let _ = self.state.gpr.insert(register, value);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{BdDisc, BdLocation, BdMenu, BdPlaylist, BdVm};
    use crate::Key;
    use disccraft_bdmv::mobj::{MovieObject, NavCommand};
    use disccraft_pgs::ig::{IgButton, IgPage};
    use std::collections::BTreeMap;

    fn object(commands: Vec<NavCommand>) -> MovieObject {
        MovieObject {
            resume_intention: false,
            menu_call_mask: false,
            title_search_mask: false,
            commands,
        }
    }

    fn button(id: u16, down: u16, commands: Vec<NavCommand>) -> IgButton {
        IgButton {
            id,
            numeric_value: id + 1,
            auto_action: false,
            x: 0,
            y: 0,
            upper: down,
            lower: down,
            left: id,
            right: id,
            normal_object: 0,
            selected_object: 0,
            activated_object: 0,
            commands: commands.iter().map(|command| command.0).collect(),
        }
    }

    /// A disc like the build writes: first playback and top menu go to the menu object (2 + titles), which picks
    /// a button and plays the menu playlist (2); the title (playlist 1, two marks) goes back to the menu when it
    /// ends; a chapter object plays it from its second mark. The title has a pop-up of one button.
    fn disc() -> BdDisc {
        let menu_object = 3;
        let chapter_object = 4;
        let menu = BdMenu {
            popup: false,
            pages: vec![IgPage {
                id: 0,
                default_selected: 0,
                palette_id: 0,
                buttons: vec![
                    button(0, 1, vec![NavCommand::jump_title(1)]),
                    button(1, 0, vec![NavCommand::jump_object(chapter_object)]),
                ],
            }],
        };
        let popup = BdMenu {
            popup: true,
            pages: vec![IgPage {
                id: 0,
                default_selected: 0,
                palette_id: 0,
                buttons: vec![button(0, 0, vec![NavCommand::jump_object(menu_object)])],
            }],
        };
        let mut playlists = BTreeMap::new();
        let _ = playlists.insert(
            1,
            BdPlaylist {
                marks: 2,
                menu: Some(popup),
            },
        );
        let _ = playlists.insert(
            2,
            BdPlaylist {
                marks: 1,
                menu: Some(menu),
            },
        );
        BdDisc {
            first_playback: 0,
            top_menu: 1,
            titles: vec![2],
            objects: vec![
                object(vec![NavCommand::jump_object(menu_object)]),
                object(vec![NavCommand::jump_object(menu_object)]),
                // Title 1: play, then back to the menu.
                object(vec![
                    NavCommand::play_pl(1),
                    NavCommand::jump_object(menu_object),
                ]),
                // The menu: the second button when GPR 2 says so, else the first; then play the menu.
                object(vec![
                    NavCommand::if_eq(2, 2),
                    NavCommand::set_button_page(1, 0),
                    NavCommand::set_reg(2, 0),
                    NavCommand::play_pl(2),
                ]),
                // Chapter 2 of title 1, then back to the menu with its second button.
                object(vec![
                    NavCommand::play_pl_mark(1, 1),
                    NavCommand::set_reg(2, 2),
                    NavCommand::jump_object(menu_object),
                ]),
            ],
            playlists,
        }
    }

    #[test]
    fn the_menu_its_buttons_the_titles_and_the_popup() -> Result<(), Box<dyn std::error::Error>> {
        let disc = disc();
        let mut vm = BdVm::start(&disc)?;
        let menu = BdLocation::Playing {
            playlist: 2,
            mark: 0,
        };
        assert_eq!(vm.state().location, menu);
        assert_eq!(vm.menu().map(|(_, view)| view.button), Some(0));

        // Down, Enter: the second button plays the title from its second mark.
        vm.press(Key::Down)?;
        assert_eq!(vm.menu().map(|(_, view)| view.button), Some(1));
        vm.press(Key::Enter)?;
        assert_eq!(
            vm.state().location,
            BdLocation::Playing {
                playlist: 1,
                mark: 1
            }
        );
        // The title's pop-up is not shown until the key.
        assert!(vm.has_popup());
        assert!(vm.menu().is_none());
        vm.popup_key();
        assert!(
            vm.menu()
                .is_some_and(|(menu, view)| menu.popup && view.open)
        );
        vm.popup_key();
        assert!(vm.menu().is_none());

        // The title ends: its chapter object sets GPR 2 and goes back to the menu, on its second button.
        vm.finish()?;
        assert_eq!(vm.state().location, menu);
        assert_eq!(vm.menu().map(|(_, view)| view.button), Some(1));
        assert!(vm.state().gpr.is_empty());

        // The first button plays title 1 from the start; the pop-up's button goes to the menu.
        vm.press(Key::Up)?;
        vm.press(Key::Enter)?;
        assert_eq!(
            vm.state().location,
            BdLocation::Playing {
                playlist: 1,
                mark: 0
            }
        );
        vm.jump_mark(1)?;
        assert_eq!(
            vm.state().location,
            BdLocation::Playing {
                playlist: 1,
                mark: 1
            }
        );
        vm.jump_mark(5)?;
        assert_eq!(
            vm.state().location,
            BdLocation::Playing {
                playlist: 1,
                mark: 1
            }
        );
        vm.popup_key();
        vm.press(Key::Enter)?;
        assert_eq!(vm.state().location, menu);

        // Top menu from anywhere; the state carries over a restore.
        vm.press(Key::Enter)?;
        let state = vm.state().clone();
        let mut again = BdVm::restore(&disc, state);
        again.top_menu()?;
        assert_eq!(again.state().location, menu);
        Ok(())
    }

    #[test]
    fn loops_and_unknown_commands_are_errors_not_hangs() {
        let mut disc = disc();
        disc.objects = vec![object(vec![NavCommand::goto_line(0)])];
        assert!(BdVm::start(&disc).is_err());
        disc.objects = vec![object(vec![NavCommand::new(0x7FFF_FFFF, 0, 0)])];
        assert!(BdVm::start(&disc).is_err());
        disc.objects = vec![object(vec![NavCommand::jump_title(9)])];
        assert!(BdVm::start(&disc).is_err());
        // An object that ends stops the disc.
        disc.objects = vec![object(vec![NavCommand::nop()])];
        assert!(BdVm::start(&disc).is_ok_and(|vm| vm.state().location == BdLocation::Stopped));
    }
}
