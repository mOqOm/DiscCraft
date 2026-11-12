//! VM instruction semantics.

use crate::VmError;
use crate::invalid;
use crate::model::ButtonModel;
use crate::model::Cmd;
use crate::model::DiscModel;
use crate::model::MenuPgcModel;
use crate::model::TitleModel;

const STEP_LIMIT: u32 = 10_000;

/// UOP10: the Title (menu call: title) key.
const UOP_TITLE_MENU: u32 = 1 << 10;
/// UOP11: the Menu (menu call: root) key.
const UOP_ROOT_MENU: u32 = 1 << 11;
/// UOP17: selecting or activating a button.
const UOP_BUTTON_SELECT: u32 = 1 << 17;

/// Current VM location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Location {
    /// A VMGM menu is displayed.
    Menu {
        /// VMGM PGC number, 1-based.
        pgcn: u16,
        /// Selected button, 1-based.
        button: u8,
    },
    /// A title is playing.
    Title {
        /// Title number, 1-based.
        title: u8,
        /// Chapter / program number, 1-based.
        chapter: u16,
    },
    /// Playback has stopped.
    Stopped,
}

/// Remote-control key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Enter / select.
    Enter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Pre,
    Post,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Context {
    FirstPlay,
    Menu {
        pgcn: u16,
        phase: Phase,
    },
    Title {
        title: u8,
        phase: Phase,
    },
    Button {
        pgcn: u16,
        button: u8,
    },
    /// The root menu of the title set of `title` (the MENU key during the title).
    RootMenu {
        title: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exec {
    Continue,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FallOff {
    Continue,
    Stop,
}

/// The VM's registers, kept by a preview between key presses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    /// General parameter registers.
    pub gprm: [u16; 16],
    /// System parameter registers (SPRM1 audio stream, SPRM2 subpicture stream and display flag, SPRM3 angle).
    pub sprm: [u16; 24],
}

impl Default for Registers {
    fn default() -> Self {
        let mut sprm = [0; 24];
        // The player's start values: no audio stream chosen (15), no subpicture (62) and hidden, angle 1.
        sprm[1] = 15;
        sprm[2] = 62;
        sprm[3] = 1;
        Self {
            gprm: [0; 16],
            sprm,
        }
    }
}

/// DVD VM simulator.
#[derive(Debug, Clone)]
pub struct Vm<'a> {
    model: &'a DiscModel,
    gprm: [u16; 16],
    sprm: [u16; 24],
    location: Location,
    steps: u32,
    highlight: Option<u8>,
    context: Context,
    ip: usize,
}

impl<'a> Vm<'a> {
    /// Start playback by running the first play PGC.
    pub fn start(model: &'a DiscModel) -> Result<Self, VmError> {
        let registers = Registers::default();
        let mut vm = Vm {
            model,
            gprm: registers.gprm,
            sprm: registers.sprm,
            location: Location::Stopped,
            steps: 0,
            highlight: None,
            context: Context::FirstPlay,
            ip: 0,
        };
        vm.run()?;
        Ok(vm)
    }

    /// Resume at a known location without re-running pre commands.
    ///
    /// This is what a preview does between key presses: the disc is already
    /// showing a menu or playing a title, so only the user input matters.
    ///
    /// # Errors
    ///
    /// Returns an error when the location refers to a menu, button, title, or
    /// chapter that the disc does not have.
    pub fn restore(model: &'a DiscModel, location: Location) -> Result<Self, VmError> {
        let registers = Registers::default();
        let mut vm = Vm {
            model,
            gprm: registers.gprm,
            sprm: registers.sprm,
            location,
            steps: 0,
            highlight: None,
            context: Context::FirstPlay,
            ip: 0,
        };

        vm.validate_location()?;
        Ok(vm)
    }

    /// [`Vm::restore`] with the general parameter registers as they were (a preview keeps them between key
    /// presses, so playlists and overrides carry on from title to title).
    ///
    /// # Errors
    ///
    /// As [`Vm::restore`].
    pub fn restore_with_registers(
        model: &'a DiscModel,
        location: Location,
        registers: Registers,
    ) -> Result<Self, VmError> {
        let mut vm = Self::restore(model, location)?;
        vm.gprm = registers.gprm;
        vm.sprm = registers.sprm;
        Ok(vm)
    }

    /// The general and system registers.
    pub fn registers(&self) -> Registers {
        Registers {
            gprm: self.gprm,
            sprm: self.sprm,
        }
    }

    /// The audio track the disc chose (1 to 8), when it chose one (SPRM1).
    pub fn audio_track(&self) -> Option<u8> {
        u8::try_from(self.sprm.get(1).copied().unwrap_or(15))
            .ok()
            .filter(|stream| *stream < 8)
            .map(|stream| stream + 1)
    }

    /// The subtitle track the disc chose (1 to 32) and whether it is shown (SPRM2).
    pub fn subtitle_track(&self) -> (bool, Option<u8>) {
        let value = self.sprm.get(2).copied().unwrap_or(62);
        let shown = value & 0x40 != 0;
        let track = u8::try_from(value & 0x3F)
            .ok()
            .filter(|stream| *stream < 32)
            .map(|stream| stream + 1);
        (shown, track)
    }

    fn validate_location(&mut self) -> Result<(), VmError> {
        match self.location {
            Location::Stopped => Ok(()),
            Location::Menu { pgcn, button } => {
                let menu = self.menu_model(pgcn)?;

                if menu.buttons.is_empty() {
                    self.location = Location::Menu { pgcn, button: 0 };
                    return Ok(());
                }

                let wanted = if button == 0 {
                    self.selected_button(menu)
                } else {
                    button
                };

                if usize::from(wanted) > menu.buttons.len() {
                    return Err(invalid("restored button out of range"));
                }

                self.location = Location::Menu {
                    pgcn,
                    button: wanted,
                };
                self.context = Context::Menu {
                    pgcn,
                    phase: Phase::Post,
                };
                Ok(())
            }
            Location::Title { title, chapter } => {
                let model = self.title_model(title)?;

                if chapter == 0 || chapter > model.chapters {
                    return Err(invalid("restored chapter out of range"));
                }

                self.context = Context::Title {
                    title,
                    phase: Phase::Post,
                };
                Ok(())
            }
        }
    }

    /// Current location.
    pub fn location(&self) -> &Location {
        &self.location
    }

    /// General parameter register value.
    pub fn gprm(&self, r: usize) -> Option<u16> {
        self.gprm.get(r).copied()
    }

    pub(crate) fn gprm_array(&self) -> [u16; 16] {
        self.gprm
    }

    /// Press a remote-control key.
    ///
    /// Arrow keys move the menu selection by the adjacency table. Enter runs the
    /// selected button command.
    pub fn press(&mut self, key: Key) -> Result<&Location, VmError> {
        let Location::Menu { pgcn, button } = self.location else {
            return Err(invalid("not in a menu"));
        };
        // A menu that prohibits selecting buttons ignores the arrows and Enter.
        if self.menu_model(pgcn)?.prohibited_ops & UOP_BUTTON_SELECT != 0 {
            return Ok(&self.location);
        }

        if key == Key::Enter {
            self.button_model(pgcn, button)?;
            self.steps = 0;
            self.highlight = None;
            self.context = Context::Button { pgcn, button };
            self.ip = 0;
            self.run()?;
            return Ok(&self.location);
        }

        let btn = self.button_model(pgcn, button)?;
        let target = match key {
            Key::Up => btn.up,
            Key::Down => btn.down,
            Key::Left => btn.left,
            Key::Right => btn.right,
            Key::Enter => 0,
        };

        self.set_button(pgcn, target)?;
        // An auto-action button runs its command as soon as it is highlighted.
        if let Location::Menu { pgcn, button } = self.location
            && button == target
            && self.button_model(pgcn, button)?.auto_action
        {
            return self.press(Key::Enter);
        }
        Ok(&self.location)
    }

    /// Press the remote's MENU key. During a title this runs the title set's root menu, which jumps to the
    /// timeline's menu remote; when the title has none, or no title is playing, nothing happens.
    pub fn menu_key(&mut self) -> Result<&Location, VmError> {
        let Location::Title { title, .. } = self.location else {
            return Ok(&self.location);
        };
        let model = self.title_model(title)?;
        if model.menu_remote.is_empty() || model.prohibited_ops & UOP_ROOT_MENU != 0 {
            return Ok(&self.location);
        }

        self.steps = 0;
        self.highlight = None;
        self.context = Context::RootMenu { title };
        self.ip = 0;
        self.run()?;
        Ok(&self.location)
    }

    /// Press the remote's TITLE key: go to the title menu, from a title or a menu. When the disc has no title
    /// menu, or playback has stopped, nothing happens.
    pub fn title_key(&mut self) -> Result<&Location, VmError> {
        let (Some(pgcn), false) = (self.model.title_menu, self.location == Location::Stopped)
        else {
            return Ok(&self.location);
        };
        // The title or menu on screen can prohibit the Title key.
        let prohibited = match self.location {
            Location::Title { title, .. } => self.title_model(title)?.prohibited_ops,
            Location::Menu { pgcn, .. } => self.menu_model(pgcn)?.prohibited_ops,
            _ => 0,
        };
        if prohibited & UOP_TITLE_MENU != 0 {
            return Ok(&self.location);
        }
        self.steps = 0;
        self.highlight = None;
        self.enter_menu(pgcn)?;
        self.run()?;
        Ok(&self.location)
    }

    /// Let the shown menu run out: a still menu's duration is over or a motion menu has played once, so its post
    /// commands (the end action) run. A menu without post commands stays on screen.
    pub fn time_out_menu(&mut self) -> Result<&Location, VmError> {
        let Location::Menu { pgcn, .. } = self.location else {
            return Err(invalid("not in a menu"));
        };
        if self.menu_model(pgcn)?.post.is_empty() {
            return Ok(&self.location);
        }

        self.steps = 0;
        self.highlight = None;
        self.context = Context::Menu {
            pgcn,
            phase: Phase::Post,
        };
        self.ip = 0;
        self.run()?;
        Ok(&self.location)
    }

    /// Finish the current title by running its post commands.
    pub fn finish_title(&mut self) -> Result<&Location, VmError> {
        let Location::Title { title, .. } = self.location else {
            return Err(invalid("not in a title"));
        };

        self.title_model(title)?;
        self.steps = 0;
        self.highlight = None;
        self.context = Context::Title {
            title,
            phase: Phase::Post,
        };
        self.ip = 0;
        self.run()?;
        Ok(&self.location)
    }

    fn run(&mut self) -> Result<(), VmError> {
        loop {
            let count = self.command_count(self.context)?;
            if self.ip >= count {
                match self.handle_fall_off()? {
                    FallOff::Stop => return Ok(()),
                    FallOff::Continue => continue,
                }
            }

            self.bump_step()?;
            let cmd = self
                .command_at(self.context, self.ip)?
                .ok_or_else(|| invalid("missing command"))?;
            self.ip = self
                .ip
                .checked_add(1)
                .ok_or_else(|| invalid("instruction pointer overflow"))?;

            match self.execute(cmd)? {
                Exec::Continue => {}
                Exec::Stop => return Ok(()),
            }
        }
    }

    fn bump_step(&mut self) -> Result<(), VmError> {
        self.steps = self.steps.checked_add(1).ok_or(VmError::Loop)?;
        if self.steps > STEP_LIMIT {
            return Err(VmError::Loop);
        }
        Ok(())
    }

    fn command_count(&self, context: Context) -> Result<usize, VmError> {
        match context {
            Context::Button { pgcn, button } => {
                self.button_model(pgcn, button)?;
                Ok(1)
            }
            _ => Ok(self.command_slice(context)?.len()),
        }
    }

    fn command_at(&self, context: Context, index: usize) -> Result<Option<Cmd>, VmError> {
        match context {
            Context::Button { pgcn, button } => {
                if index == 0 {
                    let btn = self.button_model(pgcn, button)?;
                    Ok(Some(btn.command))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(self.command_slice(context)?.get(index).copied()),
        }
    }

    fn command_slice(&self, context: Context) -> Result<&'a [Cmd], VmError> {
        match context {
            Context::FirstPlay => Ok(&self.model.first_play),
            Context::Menu { pgcn, phase } => {
                let menu = self.menu_model(pgcn)?;
                Ok(match phase {
                    Phase::Pre => &menu.pre,
                    Phase::Post => &menu.post,
                })
            }
            Context::Title { title, phase } => {
                let title = self.title_model(title)?;
                Ok(match phase {
                    Phase::Pre => &title.pre,
                    Phase::Post => &title.post,
                })
            }
            Context::Button { .. } => Err(invalid("button command is not a command table")),
            Context::RootMenu { title } => Ok(&self.title_model(title)?.menu_remote),
        }
    }

    fn handle_fall_off(&mut self) -> Result<FallOff, VmError> {
        match self.context {
            Context::FirstPlay => Err(VmError::DeadEnd),
            Context::Menu {
                pgcn,
                phase: Phase::Pre,
            } => {
                let menu = self.menu_model(pgcn)?;
                if menu.has_cells {
                    self.display_menu(pgcn)?;
                    Ok(FallOff::Stop)
                } else {
                    self.context = Context::Menu {
                        pgcn,
                        phase: Phase::Post,
                    };
                    self.ip = 0;
                    Ok(FallOff::Continue)
                }
            }
            Context::Menu {
                phase: Phase::Post, ..
            } => Err(VmError::DeadEnd),
            Context::Title {
                title,
                phase: Phase::Pre,
            } => {
                self.location = Location::Title { title, chapter: 1 };
                Ok(FallOff::Stop)
            }
            Context::Title {
                phase: Phase::Post, ..
            } => Err(VmError::DeadEnd),
            Context::Button { .. } | Context::RootMenu { .. } => Err(VmError::DeadEnd),
        }
    }

    fn display_menu(&mut self, pgcn: u16) -> Result<(), VmError> {
        let menu = self.menu_model(pgcn)?;
        let selected = self.selected_button(menu);

        if !menu.buttons.is_empty() && usize::from(selected) > menu.buttons.len() {
            return Err(invalid("selected button out of range"));
        }

        self.location = Location::Menu {
            pgcn,
            button: selected,
        };
        Ok(())
    }

    fn selected_button(&self, menu: &MenuPgcModel) -> u8 {
        if let Some(button) = self.highlight.filter(|button| *button != 0) {
            return button;
        }
        if menu.default_button != 0 {
            return menu.default_button;
        }
        1
    }

    fn menu_model(&self, pgcn: u16) -> Result<&'a MenuPgcModel, VmError> {
        let index = usize::from(pgcn)
            .checked_sub(1)
            .ok_or_else(|| invalid("PGCN is zero"))?;
        self.model
            .vmgm
            .get(index)
            .ok_or_else(|| invalid("PGCN out of range"))
    }

    fn title_model(&self, title: u8) -> Result<&'a TitleModel, VmError> {
        let index = usize::from(title)
            .checked_sub(1)
            .ok_or_else(|| invalid("title is zero"))?;
        self.model
            .titles
            .get(index)
            .ok_or_else(|| invalid("title out of range"))
    }

    fn button_model(&self, pgcn: u16, button: u8) -> Result<&'a ButtonModel, VmError> {
        let menu = self.menu_model(pgcn)?;
        let index = usize::from(button)
            .checked_sub(1)
            .ok_or_else(|| invalid("button is zero"))?;
        menu.buttons
            .get(index)
            .ok_or_else(|| invalid("button out of range"))
    }

    fn set_button(&mut self, pgcn: u16, button: u8) -> Result<(), VmError> {
        if button == 0 {
            return Ok(());
        }

        let menu = self.menu_model(pgcn)?;
        if usize::from(button) > menu.buttons.len() {
            return Err(invalid("adjacent button out of range"));
        }

        self.location = Location::Menu { pgcn, button };
        Ok(())
    }

    fn enter_menu(&mut self, pgcn: u16) -> Result<(), VmError> {
        self.menu_model(pgcn)?;
        self.context = Context::Menu {
            pgcn,
            phase: Phase::Pre,
        };
        self.ip = 0;
        self.highlight = None;
        Ok(())
    }

    fn enter_title(&mut self, title: u8) -> Result<(), VmError> {
        self.title_model(title)?;
        self.context = Context::Title {
            title,
            phase: Phase::Pre,
        };
        self.ip = 0;
        self.highlight = None;
        Ok(())
    }

    fn link_pgn(&mut self, title: u8, chapter: u16) -> Result<(), VmError> {
        let model = self.title_model(title)?;
        if chapter == 0 || chapter > model.chapters {
            return Err(invalid("chapter out of range"));
        }

        self.location = Location::Title { title, chapter };
        Ok(())
    }

    fn require_title_pre(&self) -> Result<u8, VmError> {
        match self.context {
            Context::Title {
                title,
                phase: Phase::Pre,
            } => Ok(title),
            _ => Err(invalid("LinkPGN outside title pre commands")),
        }
    }

    fn require_menu(&self) -> Result<u16, VmError> {
        match self.context {
            Context::Menu { pgcn, .. } | Context::Button { pgcn, .. } => Ok(pgcn),
            _ => Err(invalid("SetHL_BTNN outside menu context")),
        }
    }

    fn gprm_value(&self, r: u8) -> Result<u16, VmError> {
        if r >= 16 {
            return Err(invalid("GPRM out of range"));
        }
        self.gprm
            .get(usize::from(r))
            .copied()
            .ok_or_else(|| invalid("GPRM out of range"))
    }

    fn set_gprm(&mut self, r: u8, value: u16) -> Result<(), VmError> {
        if r >= 16 {
            return Err(invalid("GPRM out of range"));
        }
        let slot = self
            .gprm
            .get_mut(usize::from(r))
            .ok_or_else(|| invalid("GPRM out of range"))?;
        *slot = value;
        Ok(())
    }

    fn execute(&mut self, cmd: Cmd) -> Result<Exec, VmError> {
        let op0 = byte(&cmd, 0)?;
        let op1 = byte(&cmd, 1)?;

        // Jumps with a register compare: byte 1 = cmp << 4 | cmd, registers in bytes 6 / 7 (§8.1).
        if op0 == 0x30 && op1 & 0x80 == 0 && op1 & 0x70 != 0 {
            let holds = self.compare_registers(op1 >> 4, byte(&cmd, 6)?, byte(&cmd, 7)?)?;
            if !holds {
                return Ok(Exec::Continue);
            }
            let mut plain = cmd;
            plain[1] = op1 & 0x0F;
            plain[6] = 0;
            plain[7] = 0;
            return self.execute(plain);
        }

        match (op0, op1) {
            (0x30, 0x01) => self.exec_exit(),
            (0x30, 0x02) => self.exec_jump_tt(&cmd),
            (0x30, 0x06) => self.exec_jump_ss(&cmd),
            (0x30, 0x08) => self.exec_call_ss(&cmd),
            (0x20, 0x04) => self.exec_link_pgcn(&cmd),
            (0x20, 0x06) => self.exec_link_pgn(&cmd),
            (0x20, 0xA6) => self.exec_cond_link_pgn(&cmd),
            (0x20, 0xA4) => self.exec_cond_link_pgcn(&cmd),
            (0x00, 0xA1) => self.exec_goto_line(&cmd),
            (0x71, 0x00) => self.exec_mov_imm(&cmd),
            (0x61, 0x00) => self.exec_mov_reg(&cmd),
            (0x56, 0x00) => self.exec_set_hl(&cmd),
            (0x51, 0x01) => self.exec_set_stn(&cmd),
            _ => Err(unsupported(&cmd)),
        }
    }

    /// `GPRM[a] <op> GPRM[b]` for compare ops 1 (`&`, non-zero) … 7 (`<`).
    fn compare_registers(&self, op: u8, a: u8, b: u8) -> Result<bool, VmError> {
        let (x, y) = (self.gprm_value(a)?, self.gprm_value(b)?);
        Ok(match op {
            1 => x & y != 0,
            2 => x == y,
            3 => x != y,
            4 => x >= y,
            5 => x > y,
            6 => x <= y,
            7 => x < y,
            _ => return Err(invalid("unknown compare operator")),
        })
    }

    /// `SetSTN`: each of bytes 3 (audio), 4 (subpicture with its display flag in bit 6) and 5 (angle) replaces its
    /// register when its top bit is set.
    fn exec_set_stn(&mut self, cmd: &Cmd) -> Result<Exec, VmError> {
        let audio = byte(cmd, 3)?;
        let subpicture = byte(cmd, 4)?;
        let angle = byte(cmd, 5)?;
        if audio & 0x80 != 0
            && let Some(slot) = self.sprm.get_mut(1)
        {
            *slot = u16::from(audio & 0x07);
        }
        if subpicture & 0x80 != 0
            && let Some(slot) = self.sprm.get_mut(2)
        {
            *slot = u16::from(subpicture & 0x7F);
        }
        if angle & 0x80 != 0
            && let Some(slot) = self.sprm.get_mut(3)
        {
            *slot = u16::from(angle & 0x7F);
        }
        Ok(Exec::Continue)
    }

    fn exec_exit(&mut self) -> Result<Exec, VmError> {
        self.location = Location::Stopped;
        Ok(Exec::Stop)
    }

    fn exec_jump_tt(&mut self, cmd: &Cmd) -> Result<Exec, VmError> {
        let title = byte(cmd, 5)?;
        self.enter_title(title)?;
        Ok(Exec::Continue)
    }

    fn exec_jump_ss(&mut self, cmd: &Cmd) -> Result<Exec, VmError> {
        let pgcn = u16_be(cmd, 2)?;
        self.enter_menu(pgcn)?;
        Ok(Exec::Continue)
    }

    fn exec_call_ss(&mut self, cmd: &Cmd) -> Result<Exec, VmError> {
        let pgcn = u16_be(cmd, 2)?;
        self.enter_menu(pgcn)?;
        Ok(Exec::Continue)
    }

    fn exec_link_pgcn(&mut self, cmd: &Cmd) -> Result<Exec, VmError> {
        let pgcn = u16_be(cmd, 6)?;
        self.enter_menu(pgcn)?;
        Ok(Exec::Continue)
    }

    fn exec_link_pgn(&mut self, cmd: &Cmd) -> Result<Exec, VmError> {
        let title = self.require_title_pre()?;
        let chapter = u16::from(byte(cmd, 7)?);
        self.link_pgn(title, chapter)?;
        Ok(Exec::Stop)
    }

    fn exec_cond_link_pgn(&mut self, cmd: &Cmd) -> Result<Exec, VmError> {
        let r = byte(cmd, 3)?;
        let value = self.gprm_value(r)?;
        let compare = u16_be(cmd, 4)?;

        if value != compare {
            return Ok(Exec::Continue);
        }

        let title = self.require_title_pre()?;
        let chapter = u16::from(byte(cmd, 7)?);
        self.link_pgn(title, chapter)?;
        Ok(Exec::Stop)
    }

    fn exec_cond_link_pgcn(&mut self, cmd: &Cmd) -> Result<Exec, VmError> {
        let r = byte(cmd, 3)?;
        let value = self.gprm_value(r)?;
        let compare = u16_be(cmd, 4)?;

        if value != compare {
            return Ok(Exec::Continue);
        }

        let pgcn = u16_be(cmd, 6)?;
        self.enter_menu(pgcn)?;
        Ok(Exec::Continue)
    }

    fn exec_goto_line(&mut self, cmd: &Cmd) -> Result<Exec, VmError> {
        let r = byte(cmd, 3)?;
        let value = self.gprm_value(r)?;
        let compare = u16_be(cmd, 4)?;

        if value != compare {
            return Ok(Exec::Continue);
        }

        let line = byte(cmd, 7)?;
        if line == 0 {
            return Err(invalid("Goto line is zero"));
        }

        let count = self.command_count(self.context)?;
        let line_index = usize::from(line);
        if line_index > count {
            return Err(invalid("Goto line out of range"));
        }

        self.ip = line_index
            .checked_sub(1)
            .ok_or_else(|| invalid("Goto line underflow"))?;
        Ok(Exec::Continue)
    }

    fn exec_mov_imm(&mut self, cmd: &Cmd) -> Result<Exec, VmError> {
        let r_byte = byte(cmd, 3)?;
        if r_byte & 0xF0 != 0 {
            return Err(invalid("GPRM destination out of range"));
        }

        let value = u16_be(cmd, 4)?;
        self.set_gprm(r_byte & 0x0F, value)?;
        Ok(Exec::Continue)
    }

    fn exec_mov_reg(&mut self, cmd: &Cmd) -> Result<Exec, VmError> {
        let d_byte = byte(cmd, 3)?;
        let s = byte(cmd, 5)?;

        if d_byte & 0xF0 != 0 || s >= 16 {
            return Err(invalid("GPRM register out of range"));
        }

        let value = self.gprm_value(s)?;
        self.set_gprm(d_byte & 0x0F, value)?;
        Ok(Exec::Continue)
    }

    fn exec_set_hl(&mut self, cmd: &Cmd) -> Result<Exec, VmError> {
        let raw = byte(cmd, 4)?;
        if raw & 0x03 != 0 {
            return Err(invalid("bad SetHL_BTNN encoding"));
        }

        let button = raw >> 2;
        let pgcn = self.require_menu()?;

        if button != 0 {
            let menu = self.menu_model(pgcn)?;
            if usize::from(button) > menu.buttons.len() {
                return Err(invalid("highlight button out of range"));
            }
        }

        self.highlight = if button == 0 { None } else { Some(button) };
        Ok(Exec::Continue)
    }
}

fn byte(cmd: &Cmd, index: usize) -> Result<u8, VmError> {
    cmd.get(index)
        .copied()
        .ok_or_else(|| invalid("command out of range"))
}

fn u16_be(cmd: &Cmd, index: usize) -> Result<u16, VmError> {
    let next = index
        .checked_add(1)
        .ok_or_else(|| invalid("command index overflow"))?;
    let hi = u16::from(byte(cmd, index)?);
    let lo = u16::from(byte(cmd, next)?);
    Ok((hi << 8) | lo)
}

fn unsupported(cmd: &Cmd) -> VmError {
    VmError::Unsupported(hex_cmd(cmd))
}

fn hex_cmd(cmd: &Cmd) -> String {
    let mut out = String::with_capacity(16);
    for b in cmd {
        out.push_str(&format!("{b:02X}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::Key;
    use super::Location;
    use super::Vm;
    use crate::VmError;
    use crate::explore::explore;
    use crate::model::ButtonModel;
    use crate::model::Cmd;
    use crate::model::DiscModel;
    use crate::model::MenuPgcModel;
    use crate::model::TitleModel;

    fn button(command: Cmd, up: u8, down: u8, left: u8, right: u8) -> ButtonModel {
        ButtonModel {
            command,
            up,
            down,
            left,
            right,
            auto_action: false,
        }
    }

    fn test_model() -> DiscModel {
        let first_play: Vec<Cmd> = vec![[0x30, 0x06, 0x00, 0x01, 0x00, 0xC0, 0x00, 0x00]];

        let menu_buttons = vec![
            button([0x30, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00], 3, 2, 1, 1),
            button([0x20, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03], 1, 3, 2, 2),
            button([0x20, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04], 2, 1, 3, 3),
        ];

        let vmgm = vec![
            MenuPgcModel {
                prohibited_ops: 0,
                pre: vec![[0x71, 0x00, 0x00, 0x02, 0x00, 0x01, 0x00, 0x00]],
                post: Vec::new(),
                has_cells: true,
                buttons: menu_buttons,
                default_button: 0,
            },
            MenuPgcModel {
                prohibited_ops: 0,
                pre: vec![
                    [0x20, 0xA4, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01],
                    [0x20, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01],
                ],
                post: Vec::new(),
                has_cells: false,
                buttons: Vec::new(),
                default_button: 0,
            },
            MenuPgcModel {
                prohibited_ops: 0,
                pre: vec![
                    [0x71, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00],
                    [0x30, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00],
                ],
                post: Vec::new(),
                has_cells: false,
                buttons: Vec::new(),
                default_button: 0,
            },
            MenuPgcModel {
                prohibited_ops: 0,
                pre: vec![
                    [0x71, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00],
                    [0x30, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00],
                ],
                post: Vec::new(),
                has_cells: false,
                buttons: Vec::new(),
                default_button: 0,
            },
        ];

        let titles = vec![TitleModel {
            prohibited_ops: 0,
            pre: vec![
                [0x61, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00],
                [0x71, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
                [0x20, 0xA6, 0x00, 0x01, 0x00, 0x02, 0x00, 0x02],
                [0x20, 0xA6, 0x00, 0x01, 0x00, 0x03, 0x00, 0x03],
            ],
            post: vec![[0x30, 0x08, 0x00, 0x02, 0x00, 0xC0, 0x00, 0x00]],
            chapters: 3,
            // MENU: JumpSS VMGM pgcn 1.
            menu_remote: vec![[0x30, 0x06, 0x00, 0x01, 0x00, 0xC0, 0x00, 0x00]],
        }];

        DiscModel {
            title_menu: None,
            first_play,
            vmgm,
            titles,
        }
    }

    #[test]
    fn an_auto_action_button_runs_when_highlighted() -> Result<(), VmError> {
        let mut model = test_model();
        // Button 2 of menu 1 activates on highlight (its command is LinkPGCN 3, which plays title 1).
        if let Some(button) = model
            .vmgm
            .first_mut()
            .and_then(|menu| menu.buttons.get_mut(1))
        {
            button.auto_action = true;
        }
        let mut vm = Vm::restore(&model, Location::Menu { pgcn: 1, button: 1 })?;
        assert!(matches!(
            *vm.press(Key::Down)?,
            Location::Title { title: 1, .. }
        ));
        Ok(())
    }

    #[test]
    fn title_key_opens_the_title_menu() -> Result<(), VmError> {
        let mut model = test_model();
        model.title_menu = Some(1);
        let mut vm = Vm::restore(
            &model,
            Location::Title {
                title: 1,
                chapter: 2,
            },
        )?;
        assert!(matches!(*vm.title_key()?, Location::Menu { pgcn: 1, .. }));

        // No title menu: the key does nothing.
        let plain = test_model();
        let playing = Location::Title {
            title: 1,
            chapter: 1,
        };
        let mut vm = Vm::restore(&plain, playing)?;
        assert_eq!(*vm.title_key()?, playing);
        Ok(())
    }

    #[test]
    fn a_menu_runs_out_into_its_end_action() -> Result<(), VmError> {
        let mut model = test_model();
        // Menu 1 times out into title 1.
        if let Some(menu) = model.vmgm.first_mut() {
            menu.post = vec![[0x30, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00]];
        }
        let mut vm = Vm::restore(&model, Location::Menu { pgcn: 1, button: 1 })?;
        assert!(matches!(
            *vm.time_out_menu()?,
            Location::Title { title: 1, .. }
        ));

        // Without an end action the menu stays.
        let plain = test_model();
        let shown = Location::Menu { pgcn: 1, button: 2 };
        let mut vm = Vm::restore(&plain, shown)?;
        assert_eq!(*vm.time_out_menu()?, shown);
        Ok(())
    }

    #[test]
    fn menu_key_runs_the_root_menu() -> Result<(), VmError> {
        let model = test_model();
        let mut vm = Vm::restore(
            &model,
            Location::Title {
                title: 1,
                chapter: 2,
            },
        )?;
        assert!(matches!(*vm.menu_key()?, Location::Menu { pgcn: 1, .. }));

        // In a menu the key does nothing.
        let at_menu = *vm.location();
        assert_eq!(*vm.menu_key()?, at_menu);

        // A title without a menu remote keeps playing.
        let mut silent = model.clone();
        if let Some(title) = silent.titles.first_mut() {
            title.menu_remote.clear();
        }
        let playing = Location::Title {
            title: 1,
            chapter: 1,
        };
        let mut vm = Vm::restore(&silent, playing)?;
        assert_eq!(*vm.menu_key()?, playing);
        Ok(())
    }

    #[test]
    fn set_stn_chooses_the_streams() -> Result<(), VmError> {
        let mut model = test_model();
        // First play: audio 2, subtitle 1 shown, then the menu.
        model
            .first_play
            .insert(0, [0x51, 0x01, 0x00, 0x81, 0xC0, 0x00, 0x00, 0x00]);
        let vm = Vm::start(&model)?;
        assert_eq!(vm.audio_track(), Some(2));
        assert_eq!(vm.subtitle_track(), (true, Some(1)));
        let registers = vm.registers();
        let again = Vm::restore_with_registers(&model, *vm.location(), registers)?;
        assert_eq!(again.audio_track(), Some(2));

        // Nothing chosen: the player's start values.
        let plain_model = test_model();
        let plain = Vm::start(&plain_model)?;
        assert_eq!(plain.audio_track(), None);
        assert_eq!(plain.subtitle_track(), (false, None));
        Ok(())
    }

    #[test]
    fn prohibited_operations_ignore_the_keys() -> Result<(), VmError> {
        let mut model = test_model();
        model.title_menu = Some(1);
        if let Some(title) = model.titles.first_mut() {
            // UOP10 (Title key) and UOP11 (Menu key).
            title.prohibited_ops = (1 << 10) | (1 << 11);
        }
        if let Some(menu) = model.vmgm.first_mut() {
            // UOP17 (button select or activate).
            menu.prohibited_ops = 1 << 17;
        }
        let playing = Location::Title {
            title: 1,
            chapter: 2,
        };
        let mut vm = Vm::restore(&model, playing)?;
        assert_eq!(*vm.menu_key()?, playing);
        assert_eq!(*vm.title_key()?, playing);

        let shown = Location::Menu { pgcn: 1, button: 1 };
        let mut vm = Vm::restore(&model, shown)?;
        assert_eq!(*vm.press(Key::Down)?, shown);
        assert_eq!(*vm.press(Key::Enter)?, shown);
        Ok(())
    }

    #[test]
    fn register_compare_jump() -> Result<(), VmError> {
        // Title post: GPRM5 = 0; if (GPRM3 != GPRM5) CallSS VMGM pgcn 2; else Exit.
        let post = vec![
            [0x71, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00],
            [0x30, 0x38, 0x00, 0x02, 0x00, 0xC0, 0x03, 0x05],
            [0x30, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        ];
        let model = DiscModel {
            title_menu: None,
            first_play: vec![[0x30, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00]],
            vmgm: vec![
                MenuPgcModel {
                    prohibited_ops: 0,
                    pre: vec![],
                    post: vec![],
                    has_cells: true,
                    buttons: vec![],
                    default_button: 0,
                },
                MenuPgcModel {
                    prohibited_ops: 0,
                    pre: vec![[0x30, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00]],
                    post: vec![],
                    has_cells: false,
                    buttons: vec![],
                    default_button: 0,
                },
            ],
            titles: vec![TitleModel {
                prohibited_ops: 0,
                pre: vec![],
                post,
                chapters: 1,
                menu_remote: Vec::new(),
            }],
        };
        let mut vm = Vm::start(&model)?;
        assert_eq!(*vm.finish_title()?, Location::Stopped); // GPRM3 = 0 → falls through to Exit
        let mut vm = Vm::start(&model)?;
        vm.gprm[3] = 1;
        assert_eq!(
            *vm.finish_title()?,
            Location::Title {
                title: 1,
                chapter: 1
            }
        ); // enters pgcn 2 → JumpTT 1
        Ok(())
    }

    #[test]
    fn title_menu_chapter_flow() -> Result<(), VmError> {
        let model = test_model();
        let mut vm = Vm::start(&model)?;

        assert_eq!(vm.location(), &Location::Menu { pgcn: 1, button: 1 });

        vm.press(Key::Enter)?;
        assert_eq!(
            vm.location(),
            &Location::Title {
                title: 1,
                chapter: 1
            }
        );

        vm.finish_title()?;
        assert_eq!(vm.location(), &Location::Menu { pgcn: 1, button: 1 });

        vm.press(Key::Down)?;
        assert_eq!(vm.location(), &Location::Menu { pgcn: 1, button: 2 });
        vm.press(Key::Enter)?;
        assert_eq!(
            vm.location(),
            &Location::Title {
                title: 1,
                chapter: 2
            }
        );

        vm.finish_title()?;
        assert_eq!(vm.location(), &Location::Menu { pgcn: 1, button: 1 });

        vm.press(Key::Down)?;
        vm.press(Key::Down)?;
        assert_eq!(vm.location(), &Location::Menu { pgcn: 1, button: 3 });
        vm.press(Key::Enter)?;
        assert_eq!(
            vm.location(),
            &Location::Title {
                title: 1,
                chapter: 3
            }
        );

        Ok(())
    }

    #[test]
    fn explore_reaches_all_chapters() -> Result<(), VmError> {
        let model = test_model();
        let report = explore(&model);

        let mut expected_menus = BTreeSet::new();
        expected_menus.insert(1u16);

        let mut expected_chapters = BTreeSet::new();
        expected_chapters.insert((1u8, 1u16));
        expected_chapters.insert((1u8, 2u16));
        expected_chapters.insert((1u8, 3u16));

        assert_eq!(report.menus_seen, expected_menus);
        assert_eq!(report.chapters_reached, expected_chapters);
        assert!(report.dead_ends.is_empty());
        assert!(report.loops.is_empty());

        Ok(())
    }

    #[test]
    fn self_link_loops() -> Result<(), VmError> {
        let model = DiscModel {
            title_menu: None,
            first_play: vec![[0x30, 0x06, 0x00, 0x01, 0x00, 0xC0, 0x00, 0x00]],
            vmgm: vec![MenuPgcModel {
                prohibited_ops: 0,
                pre: vec![[0x20, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01]],
                post: Vec::new(),
                has_cells: false,
                buttons: Vec::new(),
                default_button: 0,
            }],
            titles: Vec::new(),
        };

        match Vm::start(&model) {
            Err(VmError::Loop) => Ok(()),
            Err(other) => Err(other),
            Ok(_) => Err(VmError::Invalid("expected loop".to_string())),
        }
    }

    #[test]
    fn hidden_pgc_without_jump_is_dead_end() -> Result<(), VmError> {
        let model = DiscModel {
            title_menu: None,
            first_play: vec![[0x30, 0x06, 0x00, 0x01, 0x00, 0xC0, 0x00, 0x00]],
            vmgm: vec![MenuPgcModel {
                prohibited_ops: 0,
                pre: Vec::new(),
                post: Vec::new(),
                has_cells: false,
                buttons: Vec::new(),
                default_button: 0,
            }],
            titles: Vec::new(),
        };

        match Vm::start(&model) {
            Err(VmError::DeadEnd) => Ok(()),
            Err(other) => Err(other),
            Ok(_) => Err(VmError::Invalid("expected dead end".to_string())),
        }
    }

    #[test]
    fn restore_resumes_a_menu_without_pre_commands() -> Result<(), VmError> {
        let model = test_model();

        // A preview can resume at any menu/button, not only where first play lands.
        let mut vm = Vm::restore(&model, Location::Menu { pgcn: 1, button: 3 })?;
        assert_eq!(vm.location(), &Location::Menu { pgcn: 1, button: 3 });

        vm.press(Key::Up)?;
        assert_eq!(vm.location(), &Location::Menu { pgcn: 1, button: 2 });

        Ok(())
    }

    #[test]
    fn restore_resumes_a_title() -> Result<(), VmError> {
        // A disc whose title returns to the menu when it ends.
        let model = DiscModel {
            title_menu: None,
            first_play: vec![[0x30, 0x06, 0x00, 0x01, 0x00, 0xC0, 0x00, 0x00]],
            vmgm: vec![MenuPgcModel {
                prohibited_ops: 0,
                pre: Vec::new(),
                post: Vec::new(),
                has_cells: true,
                buttons: vec![button(
                    [0x30, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00],
                    1,
                    1,
                    1,
                    1,
                )],
                default_button: 0,
            }],
            titles: vec![TitleModel {
                prohibited_ops: 0,
                pre: Vec::new(),
                post: vec![[0x30, 0x08, 0x00, 0x01, 0x00, 0xC0, 0x00, 0x00]],
                chapters: 4,
                menu_remote: Vec::new(),
            }],
        };

        let mut vm = Vm::restore(
            &model,
            Location::Title {
                title: 1,
                chapter: 2,
            },
        )?;

        assert_eq!(
            vm.location(),
            &Location::Title {
                title: 1,
                chapter: 2
            }
        );

        // Title post commands return to the menu.
        assert_eq!(*vm.finish_title()?, Location::Menu { pgcn: 1, button: 1 });

        Ok(())
    }

    #[test]
    fn restore_rejects_unknown_targets() {
        let model = test_model();

        assert!(
            Vm::restore(&model, Location::Menu { pgcn: 9, button: 1 }).is_err(),
            "unknown menu"
        );
        assert!(
            Vm::restore(&model, Location::Menu { pgcn: 1, button: 9 }).is_err(),
            "unknown button"
        );
        assert!(
            Vm::restore(
                &model,
                Location::Title {
                    title: 1,
                    chapter: 99
                }
            )
            .is_err(),
            "unknown chapter"
        );
    }

    #[test]
    fn unknown_command_is_unsupported() -> Result<(), VmError> {
        let model = DiscModel {
            title_menu: None,
            first_play: vec![[0xFF; 8]],
            vmgm: Vec::new(),
            titles: Vec::new(),
        };

        match Vm::start(&model) {
            Err(VmError::Unsupported(_)) => Ok(()),
            Err(other) => Err(other),
            Ok(_) => Err(VmError::Invalid("expected unsupported".to_string())),
        }
    }
}
