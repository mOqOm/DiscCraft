//! The menu bar on craft-libs' craft-menu, as the other Crafting Apps draw theirs (M16.4).
//!
//! The menus' contents are written once, against [`MenuSink`]: [`ModelSink`] turns them into craft-menu's model, which
//! [`craft_menu::layout`] lays out the way each platform's apps do (on Windows and Linux About goes last in Help,
//! Preferences last in Edit and Exit last in File; on macOS they go to the app menu with the language choice) and
//! [`craft_menu::MenuBarUi`] draws, with its keyboard access (a tap on Alt or F10 puts the keys on the bar), mnemonics,
//! scrolling and overflow. [`EguiSink`] draws the same rows straight into an egui menu, for the menu canvas's context
//! menu, which shares the Edit and Object rows.

use crate::actions::{Action, Trigger, menu_check, menu_item};
use crate::i18n::Language;
use crate::workspace::WorkspaceId;
use craft_menu::{Chord, CommandRef, Item, ItemRole, Menu, MenuBar, MenuRole, Node, Platforms};
use disccraft_project::AssetId;
use egui::Ui;

/// What a menu row runs: an [`Action`] (as its shortcut does), or a choice of a list the menu shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MenuCommand {
    Action(Action),
    /// File ▸ a recent project.
    OpenRecent(std::path::PathBuf),
    /// Window ▸ Workspace ▸ a workspace.
    Workspace(WorkspaceId),
    /// Window ▸ Workspace ▸ New Workspace…
    NewWorkspace,
    /// Window ▸ Workspace ▸ Delete Workspace ▸ a saved workspace.
    DeleteWorkspace(String),
    /// Window ▸ Workspace ▸ Reset.
    ResetWorkspace,
    /// The interface language.
    Language(Language),
    /// Edit ▸ History ▸ a step: undo or redo that many steps.
    History {
        undo: bool,
        steps: usize,
    },
    /// File ▸ Render ▸ a motion menu or a slideshow.
    Render(disccraft_build::Render),
    /// Timeline ▸ New Timeline ▸ a video.
    CreateTimeline(AssetId),
    /// Timeline ▸ Export Subtitles ▸ a subtitle track's asset.
    ExportSubtitles(AssetId),
}

/// Where the menus' rows go: craft-menu's model ([`ModelSink`]) or an egui menu ([`EguiSink`]).
pub(crate) trait MenuSink {
    /// An action's row, with its label and shortcut.
    fn action(&mut self, action: Action, enabled: bool);
    /// An action's row under another label (the shortcut stays the action's).
    fn action_as(&mut self, label: &str, action: Action, enabled: bool);
    /// An action's check row.
    fn check(&mut self, action: Action, checked: bool);
    /// A row that runs `command`; `radio` makes it one choice of a group (selected or not).
    fn command(&mut self, label: String, command: MenuCommand, enabled: bool, radio: Option<bool>);
    /// A row that only says something (greyed).
    fn note(&mut self, text: &str);
    fn separator(&mut self);
    /// A submenu; `build` writes its rows.
    fn submenu(&mut self, label: &str, enabled: bool, build: &mut dyn FnMut(&mut dyn MenuSink));
    /// A submenu shown only where `platforms` says, with a role the layouts put in a fixed place.
    fn submenu_with(
        &mut self,
        label: &str,
        role: Option<ItemRole>,
        platforms: Platforms,
        build: &mut dyn FnMut(&mut dyn MenuSink),
    ) {
        let _ = (role, platforms);
        self.submenu(label, true, build);
    }
}

/// The commands of the rows [`ModelSink`] wrote, by the index each row's [`CommandRef`] carries.
#[derive(Default)]
pub(crate) struct Commands {
    rows: Vec<MenuCommand>,
}

/// The id of every row's command; its `row` parameter is the index in [`Commands`].
const ROW: &str = "disccraft.menu";

impl Commands {
    fn add(&mut self, command: MenuCommand) -> CommandRef {
        let index = self.rows.len();
        self.rows.push(command);
        CommandRef::with_params(ROW, serde_json::json!({ "row": index }))
    }

    /// The command of a chosen row.
    pub(crate) fn get(&self, command: &CommandRef) -> Option<&MenuCommand> {
        if command.id != ROW {
            return None;
        }
        let index = command.params.get("row")?.as_u64()?;
        self.rows.get(usize::try_from(index).ok()?)
    }
}

/// Builds craft-menu's rows.
pub(crate) struct ModelSink<'c> {
    nodes: Vec<Node>,
    commands: &'c mut Commands,
}

impl<'c> ModelSink<'c> {
    pub(crate) fn new(commands: &'c mut Commands) -> Self {
        Self {
            nodes: Vec::new(),
            commands,
        }
    }

    /// The rows written, as a top-level menu. A translated title carries its English name, whose letter the bar takes
    /// as its mnemonic ("ファイル(F)").
    pub(crate) fn menu(self, title: &str, role: MenuRole) -> Menu {
        let mut nodes = self.nodes;
        craft_menu::tidy(&mut nodes);
        let menu = Menu::new(title, nodes).role(role);
        match english_title(title) {
            Some(english) if english != title => menu.english(english),
            _ => menu,
        }
    }

    fn action_item(&mut self, label: &str, action: Action, enabled: bool) -> Item {
        let english = english_label(action);
        let command = self.commands.add(MenuCommand::Action(action));
        let mut item = Item::new(command, label).enabled(enabled);
        // A translated label carries its English one, whose letter the menu takes as the row's mnemonic.
        if english != label {
            item = item.english(english);
        }
        if let Some(chord) = chord(action) {
            item = item.shortcut(chord);
        }
        match action {
            Action::About => item.role(ItemRole::About),
            Action::Preferences => item.role(ItemRole::Settings),
            Action::Exit => item.role(ItemRole::Quit),
            _ => item,
        }
    }
}

impl MenuSink for ModelSink<'_> {
    fn action(&mut self, action: Action, enabled: bool) {
        let item = self.action_item(action.label(), action, enabled);
        self.nodes.push(item.into());
    }

    fn action_as(&mut self, label: &str, action: Action, enabled: bool) {
        let item = self.action_item(label, action, enabled);
        self.nodes.push(item.into());
    }

    fn check(&mut self, action: Action, checked: bool) {
        let item = self
            .action_item(action.label(), action, true)
            .check(checked);
        self.nodes.push(item.into());
    }

    fn command(&mut self, label: String, command: MenuCommand, enabled: bool, radio: Option<bool>) {
        let command = self.commands.add(command);
        let mut item = Item::new(command, label).enabled(enabled);
        if let Some(selected) = radio {
            item = item.radio(selected);
        }
        self.nodes.push(item.into());
    }

    fn note(&mut self, text: &str) {
        let command = self.commands.add(MenuCommand::Action(Action::Help));
        self.nodes
            .push(Item::new(command, text).enabled(false).into());
    }

    fn separator(&mut self) {
        self.nodes.push(Node::Separator);
    }

    fn submenu(&mut self, label: &str, enabled: bool, build: &mut dyn FnMut(&mut dyn MenuSink)) {
        let mut child = ModelSink::new(self.commands);
        build(&mut child);
        let mut children = child.nodes;
        if !enabled {
            // A greyed submenu: its rows stay, unchosen.
            for node in &mut children {
                disable(node);
            }
        }
        self.nodes.push(Node::submenu(label, children));
    }

    fn submenu_with(
        &mut self,
        label: &str,
        role: Option<ItemRole>,
        platforms: Platforms,
        build: &mut dyn FnMut(&mut dyn MenuSink),
    ) {
        let mut child = ModelSink::new(self.commands);
        build(&mut child);
        let mut node = Node::submenu(label, child.nodes).platforms(platforms);
        if let Some(role) = role {
            node = node.role(role);
        }
        self.nodes.push(node);
    }
}

/// Greys a row and everything under it.
fn disable(node: &mut Node) {
    match node {
        Node::Item(item) => item.enabled = false,
        Node::Submenu { children, .. } => children.iter_mut().for_each(disable),
        _ => {}
    }
}

/// Draws the rows straight into an egui menu (a context menu), collecting what was chosen.
pub(crate) struct EguiSink<'u> {
    pub(crate) ui: &'u mut Ui,
    pub(crate) chosen: Vec<MenuCommand>,
}

impl<'u> EguiSink<'u> {
    pub(crate) fn new(ui: &'u mut Ui) -> Self {
        Self {
            ui,
            chosen: Vec::new(),
        }
    }
}

impl MenuSink for EguiSink<'_> {
    fn action(&mut self, action: Action, enabled: bool) {
        if menu_item(self.ui, action, enabled) {
            self.chosen.push(MenuCommand::Action(action));
        }
    }

    fn action_as(&mut self, label: &str, action: Action, enabled: bool) {
        let button = egui::Button::new(label).shortcut_text(action.shortcut_text(self.ui.ctx()));
        if self.ui.add_enabled(enabled, button).clicked() {
            self.chosen.push(MenuCommand::Action(action));
        }
    }

    fn check(&mut self, action: Action, checked: bool) {
        if menu_check(self.ui, action, checked) {
            self.chosen.push(MenuCommand::Action(action));
        }
    }

    fn command(&mut self, label: String, command: MenuCommand, enabled: bool, radio: Option<bool>) {
        let response = match radio {
            Some(selected) => self
                .ui
                .add_enabled(enabled, egui::Button::selectable(selected, label)),
            None => self.ui.add_enabled(enabled, egui::Button::new(label)),
        };
        if response.clicked() {
            self.chosen.push(command);
        }
    }

    fn note(&mut self, text: &str) {
        self.ui.label(text);
    }

    fn separator(&mut self) {
        self.ui.separator();
    }

    fn submenu(&mut self, label: &str, enabled: bool, build: &mut dyn FnMut(&mut dyn MenuSink)) {
        let mut chosen = Vec::new();
        self.ui.add_enabled_ui(enabled, |ui| {
            ui.menu_button(label, |ui| {
                let mut child = EguiSink::new(ui);
                build(&mut child);
                chosen = child.chosen;
            });
        });
        self.chosen.extend(chosen);
    }
}

/// The action's label in English, for the mnemonics of its translated label.
fn english_label(action: Action) -> String {
    let before = crate::i18n::current();
    crate::i18n::set_current(Language::En);
    let label = action.label().to_owned();
    crate::i18n::set_current(before);
    label
}

/// The action's shortcut as craft-menu shows it, in the target's names (Ctrl+S on Windows, ⌘S on a Mac).
pub(crate) fn chord(action: Action) -> Option<Chord> {
    match action.trigger()? {
        Trigger::Key(shortcut) => {
            let key = craft_keymap::Key::from_egui(shortcut.logical_key)?;
            let mods = craft_keymap::Mods::from_egui(
                &shortcut.modifiers,
                craft_keymap::Platform::current(),
            );
            Some(Chord::new(mods, key))
        }
        Trigger::Text(text) => Chord::parse(text).ok(),
        Trigger::KeypadShifted(text) => Chord::parse(&format!("Shift+{text}")).ok(),
    }
}

/// The labels craft-menu's layouts and bar write themselves (`About {app}`, `Preferences…`, `Exit`, `⋯`), translated.
pub(crate) fn layout_label(english: &'static str) -> String {
    crate::i18n::t(english).to_owned()
}

/// The menu bar's model: its top-level menus, in Encore's order (File, Edit, Object, Menu, Timeline, View, Window,
/// Help), with the roles the layouts need.
pub(crate) fn bar(menus: Vec<Menu>) -> MenuBar {
    MenuBar::new(menus)
}

/// The English titles of the top-level menus, for the mnemonics of the translated ones.
pub(crate) fn english_title(title: &str) -> Option<String> {
    [
        "File", "Edit", "Object", "Menu", "Timeline", "View", "Window", "Help",
    ]
    .into_iter()
    .find(|english| crate::i18n::t(english) == title)
    .map(str::to_owned)
}
