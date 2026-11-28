//! craft-libs' UI runtime, which craft-ui's docking and craft-menu's menu bar are built on (M9.54), hosted from
//! DiscCraft's own eframe `App` through the `craft_ui::shell::embed` hooks (craft-libs docs/craft-framework.md §2.2):
//! [`install`] once when the app is created, [`before_input`] in `raw_input_hook`, [`close_path`] in `logic` and
//! [`before_ui`] first in `ui`. FilmCraft hosts it the same way (its `crates/ui-egui/src/framework.rs`,
//! https://github.com/storytold/filmcraft rev `48fce6769713ae38a6a889ec87d38da8db3d793c`, MIT OR Apache-2.0).
//!
//! The runtime takes some keys for the whole window. DiscCraft keeps Encore's: the runtime's own commands whose keys
//! DiscCraft binds (F6 Layers and F10 Progress, Ctrl+Q Exit, the edit chords, which DiscCraft reads from egui) are
//! unbound ([`OVERRIDES`]), so those keys stay in egui's input. An Escape that no framework component takes (a menu of
//! the bar, a dock gesture) is handed back to egui ([`give_back_escape`]), so DiscCraft's dialogs, drags and fields take
//! it back as before.

use std::sync::{Arc, LazyLock, Mutex};

use craft_host::target::ShellInfo;
use craft_host::{AppInfo, Host};
use craft_keymap::Overrides;
use craft_theme::Mode;
use craft_ui::InstallSpec;
use craft_ui::runtime::commands::{
    ChromeCommand, CommandInput, CommandSet as _, EditCommands, NavCommand, QuitCommand,
};
use craft_ui::runtime::env::{RootEnv, Scale};
use craft_ui::runtime::frame::FrameInput;
use craft_ui::runtime::keys::{Intent, with_window_intents};
use craft_ui::shell::embed::{self, CloseDecision};
use craft_window::system::WindowSystem;
use egui::Context;

use crate::i18n::Language;

/// The runtime's commands whose keys are DiscCraft's own, unbound so the keys reach egui (and DiscCraft's shortcuts):
/// F6 and Shift+F6 (Encore's Window ▸ Layers), F10 (Window ▸ Progress), Ctrl+Q (File ▸ Exit, which asks about unsaved
/// work), the edit chords and Delete (DiscCraft's Edit menu reads them from egui) and Tab (a field's focus move, not
/// the panels' visibility). One `Arc` for the app's life: the runtime rebuilds its keymap when the `Arc` changes.
static OVERRIDES: LazyLock<Arc<Overrides>> = LazyLock::new(|| {
    let mut overrides = Overrides::new();
    let unbound = [
        NavCommand::NextPane.id(),
        NavCommand::PreviousPane.id(),
        NavCommand::FocusMenuBar.id(),
        QuitCommand::Quit.id(),
        ChromeCommand::TogglePanels.id(),
        ChromeCommand::TogglePanelsKeepTools.id(),
    ]
    .into_iter()
    .chain(EditCommands::ALL.into_iter().map(|command| command.id()));
    for id in unbound {
        overrides.insert(id, Vec::new());
    }
    Arc::new(overrides)
});

/// DiscCraft's sizes over the suite's Compact set (craft-libs docs/craft-framework.md §4.6, step 2): Encore's heights,
/// which DiscCraft kept before the runtime: a tab strip 24 points high, a menu bar 23 (its titles 19) and grooves of 4
/// between the groups. Built once, so the runtime's token cache keeps it.
static SIZES: LazyLock<Arc<craft_theme::AppSizes>> = LazyLock::new(|| {
    use craft_theme::SizeValue::Number;
    let mut sizes = craft_theme::AppSizes::default();
    sizes.all.insert(
        "size.panel_header".to_owned(),
        Number(crate::theme::TAB_HEIGHT),
    );
    sizes
        .all
        .insert("size.control.md".to_owned(), Number(MENU_TITLE_HEIGHT));
    // The gutter between the panels' groups.
    sizes
        .all
        .insert("space.gap.tight".to_owned(), Number(GROUP_GAP));
    Arc::new(sizes)
});

/// The height of the menu bar's titles (a control's height in the suite's sizes).
const MENU_TITLE_HEIGHT: f32 = 19.0;
/// The gutter between the panels' groups (Encore's dark grooves).
const GROUP_GAP: f32 = 4.0;

/// DiscCraft's colours for the framework's own parts (§4.6, step 4): the suite's default theme (ArtCraft's seeds) with
/// Encore's tab strip, panel, grooves and border, as DiscCraft drew its docking before the runtime, in both looks. `None` when
/// the theme does not read (the suite's default theme draws then, and the log says why).
static THEME: LazyLock<Option<Arc<craft_theme::ThemeDef>>> =
    LazyLock::new(
        || match craft_theme::format::parse(theme_json().as_bytes()) {
            Ok(file) => Some(Arc::new(file.theme)),
            Err(errors) => {
                crate::help::log(&format!("DiscCraft's UI theme does not read: {errors:?}"));
                None
            }
        },
    );

/// The theme file of [`THEME`] (craft-libs docs/suite-themes.md): DiscCraft's colours pinned over ArtCraft's seeds.
fn theme_json() -> String {
    use crate::theme::{BORDER, GUTTER, PANEL, Shade, TAB_STRIP};
    let hex =
        |color: egui::Color32| format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b());
    let pins = |look: fn(Shade) -> egui::Color32| {
        serde_json::json!({
            "surface.background": hex(look(TAB_STRIP)),
            "surface.panel": hex(look(PANEL)),
            "surface.canvas": hex(look(GUTTER)),
            "line.border": hex(look(BORDER)),
        })
    };
    serde_json::json!({
        "schema": "craft-design-theme",
        "schema_version": 1,
        "id": "disccraft.encore",
        "name": "DiscCraft",
        "variants": "both",
        "seeds": {
            "neutral": "#535353",
            "accent": "#78adf6",
            "status": { "good": "#85dc6a", "warn": "#e8b046", "bad": "#ff8080", "info": "#78adf6" }
        },
        "light": pins(|shade| shade.light),
        "dark": pins(|shade| shade.dark),
    })
    .to_string()
}

/// The app's host for the runtime: its name and version, and the interface language with DiscCraft's catalogs, so the
/// framework's own words (the dock's, the menu bar's) read in the language the interface is drawn in. One per
/// language, kept: the runtime takes a new host as a change.
fn host(language: Language) -> Arc<Host> {
    static HOSTS: Mutex<Vec<(Language, Arc<Host>)>> = Mutex::new(Vec::new());
    let Ok(mut hosts) = HOSTS.lock() else {
        return Arc::new(build_host(language));
    };
    if let Some((_, host)) = hosts.iter().find(|(kept, _)| *kept == language) {
        return Arc::clone(host);
    }
    let host = Arc::new(build_host(language));
    hosts.push((language, Arc::clone(&host)));
    host
}

fn build_host(language: Language) -> Host {
    let (i18n, lang) = crate::i18n::shared(language);
    Host::builder(AppInfo {
        id: "disccraft",
        name: "DiscCraft",
        version: env!("CARGO_PKG_VERSION"),
    })
    .i18n(i18n)
    .lang(lang)
    // The icon the update dialogs show (M9.65).
    .icon_png(include_bytes!(
        "../../../assets/app-icon/hicolor/128x128/apps/io.github.mOqOm.DiscCraft.png"
    ))
    .build()
}

/// Installs the runtime on the desktop app's context, with the platform's window services. Call it once from the app
/// creator, before the fonts ([`crate::install_fonts`] hands its fonts to the runtime). A context that has the runtime
/// already keeps it.
pub fn install(ctx: &Context) {
    #[cfg(not(target_arch = "wasm32"))]
    let system: Arc<dyn WindowSystem> = Arc::new(craft_window::system::SystemWindows);
    #[cfg(target_arch = "wasm32")]
    let system: Arc<dyn WindowSystem> = Arc::new(craft_window::system::NoWindows);
    let spec = InstallSpec::new(desktop_shell(), system).app_id("io.github.mOqOm.DiscCraft");
    install_with(ctx, spec);
}

/// The desktop's shell: the platform's backend (where the window host opens OS windows).
fn desktop_shell() -> ShellInfo {
    #[cfg(not(target_arch = "wasm32"))]
    {
        ShellInfo::default().backend(craft_host::target::Backend::from_env())
    }
    #[cfg(target_arch = "wasm32")]
    {
        ShellInfo::default()
    }
}

/// Installs the runtime with no window services: every test that runs the app's UI. A context that has the runtime
/// already keeps it.
#[cfg(test)]
pub(crate) fn install_headless(ctx: &Context) {
    install_with(
        ctx,
        InstallSpec::new(
            ShellInfo::default(),
            Arc::new(craft_window::system::NoWindows),
        ),
    );
}

fn install_with(ctx: &Context, spec: InstallSpec) {
    // DiscCraft's own input hook goes first, so it sees each window's input before the runtime does.
    if ctx.plugin_opt::<WindowInputs>().is_none() {
        ctx.add_plugin(WindowInputs::default());
    }
    match craft_ui::install(ctx, spec) {
        Ok(()) | Err(craft_ui::InstallError::AlreadyInstalled) => {}
        // The app still runs without it: the dock and the menu bar draw nothing then, and the log says why.
        Err(error) => crate::help::log(&format!("the UI runtime could not be installed: {error}")),
    }
}

/// From `raw_input_hook`, before each frame: the theme's mode, the interface scale (egui's zoom factor, which the
/// runtime writes back unchanged), the host and DiscCraft's keys ([`OVERRIDES`]). A Ctrl+wheel stays a wheel (the
/// timeline and the monitor zoom with it at their own rate), not a pinch. Returns the commands the runtime fired since
/// the last frame: the rows chosen in the menu bar's menus.
pub(crate) fn before_input(ctx: &Context, language: Language) -> Vec<craft_keymap::CommandRef> {
    let mode = if crate::theme::is_light() {
        Mode::Light
    } else {
        Mode::Dark
    };
    let mut root = RootEnv::new()
        .mode(mode)
        .scale(Scale::new(ctx.zoom_factor()))
        .ctrl_wheel_pinch(false);
    root.sizes = Arc::clone(&SIZES);
    if let Some(theme) = THEME.as_ref() {
        root = root.library_theme(Arc::clone(theme));
    }
    let mut fired = Vec::new();
    embed::before_input(ctx, |output| {
        fired = output
            .fired
            .into_iter()
            .map(|fired| fired.command)
            .collect();
        FrameInput::new(root)
            .commands(CommandInput::new().overrides(Arc::clone(&OVERRIDES)))
            .host(host(language))
    });
    fired
}

/// First in `ui`, each pass of the main window: hands the runtime the native window (eframe's `Frame`).
pub fn before_ui(ctx: &Context, frame: &eframe::Frame) {
    #[cfg(not(target_arch = "wasm32"))]
    embed::before_ui(ctx, frame);
    #[cfg(target_arch = "wasm32")]
    let _ = (ctx, frame);
}

/// From `logic`, after the app's own logic: the main window's close path. `decide` answers a close request (the
/// window's ×, Alt+F4): [`CloseDecision::Hold`] keeps the window while DiscCraft asks about unsaved work.
pub fn close_path(ctx: &Context, decide: impl FnOnce() -> CloseDecision) {
    embed::logic(ctx, decide);
}

/// First in each window's UI, each pass (the main window, the Project Preview, the video output): the window is
/// DiscCraft's own document canvas for the UI runtime, so while no field or framework component has the focus the keys
/// stay DiscCraft's: the clipboard's events and the edit chords (Edit ▸ Cut, Copy, Paste, Undo …), the arrows (a
/// menu's objects nudged, the timeline's playhead, the remote's arrows). The canvas is a widget with no size (it takes no
/// pointer), which the runtime's Tab order keeps.
pub(crate) fn host_window(ui: &mut egui::Ui) {
    use craft_ui::runtime::focus::{EditSet, KeyUse};
    let id = egui::Id::unique("disccraft_window_keys").with(ui.ctx().viewport_id());
    let at = egui::Rect::from_min_size(ui.max_rect().min, egui::Vec2::ZERO);
    let _ = ui.interact(at, id, egui::Sense::click());
    craft_ui::runtime::focus::host_canvas(ui.ctx(), id, KeyUse::CANVAS.with_edits(EditSet::ALL));
}

/// What DiscCraft takes from each window's raw input before the UI runtime reads it (M9.55): an egui plugin, registered
/// before the runtime so that its input hook runs first, in every window (the main one and the panels floated into OS
/// windows of their own).
///
/// - A right click on a panel's tab (as the window last drew its tabs) is taken, with its release: DiscCraft shows its
///   own menu of that panel ([`crate::dock`]) in place of craft-ui's group menu, whose rows craft-ui names in English
///   only.
/// - An Escape press is noted, before the runtime routes it, for [`give_back_escape`]. (FilmCraft notes it the same
///   way: its `note_escape`.)
/// - A window that loses the keyboard with a menu of the bar open closes the menu (an Escape after the focus event).
#[derive(Default)]
pub(crate) struct WindowInputs {
    /// The tabs each window drew in its last pass: the window, the panel and the tab's rect in the window's points.
    tabs: Vec<(egui::ViewportId, crate::workspace::Panel, egui::Rect)>,
    /// A right click on a tab, taken: the window, the panel and where it was.
    tab_menu: Option<(egui::ViewportId, crate::workspace::Panel, egui::Pos2)>,
    /// The windows whose right-click release is to be taken too.
    swallow: Vec<egui::ViewportId>,
    /// The windows whose input of this frame carries an Escape press.
    escapes: Vec<egui::ViewportId>,
}

impl egui::Plugin for WindowInputs {
    fn debug_name(&self) -> &'static str {
        "DiscCraft window inputs"
    }

    fn input_hook(&mut self, ctx: &Context, input: &mut egui::RawInput) {
        let viewport = input.viewport_id;
        // The window lost the keyboard to another one with a menu of the bar open: the menu closes, as a Windows menu
        // does. (Left open, the runtime's menu no longer took its letters once the window had the keyboard back.)
        let blurred = input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::WindowFocused(false)));
        if blurred && craft_menu::bar::is_open(ctx) {
            for pressed in [true, false] {
                input.events.push(egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: Some(egui::Key::Escape),
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
        }
        let escape = input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    repeat: false,
                    ..
                }
            )
        });
        self.escapes.retain(|other| *other != viewport);
        if escape {
            self.escapes.push(viewport);
        }
        let tabs = &self.tabs;
        let swallow = &mut self.swallow;
        let mut taken = None;
        input.events.retain(|event| match event {
            egui::Event::PointerButton {
                button: egui::PointerButton::Secondary,
                pressed: true,
                pos,
                ..
            } => match tabs
                .iter()
                .find(|(window, _, rect)| *window == viewport && rect.contains(*pos))
            {
                Some((_, panel, _)) => {
                    taken = Some((viewport, *panel, *pos));
                    if !swallow.contains(&viewport) {
                        swallow.push(viewport);
                    }
                    false
                }
                None => true,
            },
            egui::Event::PointerButton {
                button: egui::PointerButton::Secondary,
                pressed: false,
                ..
            } if swallow.contains(&viewport) => {
                swallow.retain(|other| *other != viewport);
                false
            }
            _ => true,
        });
        if taken.is_some() {
            self.tab_menu = taken;
        }
    }
}

/// The tabs every window drew in this pass (the window, the panel, the tab's rect in the window's points): a right
/// click on one of them is DiscCraft's in the next frames.
pub(crate) fn set_tabs(
    ctx: &Context,
    tabs: Vec<(egui::ViewportId, crate::workspace::Panel, egui::Rect)>,
) {
    let _ = ctx.with_plugin(|inputs: &mut WindowInputs| inputs.tabs = tabs);
}

/// Where `panel`'s tab was in the last pass that drew it, in its window's points (a floating window's tabs are drawn
/// before DiscCraft sees this pass's rects, so its chrome goes where the tab was a pass ago).
pub(crate) fn tab_rect(ctx: &Context, panel: crate::workspace::Panel) -> Option<egui::Rect> {
    ctx.with_plugin(|inputs: &mut WindowInputs| {
        inputs
            .tabs
            .iter()
            .find_map(|(_, drawn, rect)| (*drawn == panel).then_some(*rect))
    })
    .flatten()
}

/// The right click on a tab of `window` taken from its input, if any: the panel and where it was.
pub(crate) fn take_tab_menu(
    ctx: &Context,
    window: egui::ViewportId,
) -> Option<(crate::workspace::Panel, egui::Pos2)> {
    ctx.with_plugin(|inputs: &mut WindowInputs| match inputs.tab_menu {
        Some((at, panel, pos)) if at == window => {
            inputs.tab_menu = None;
            Some((panel, pos))
        }
        _ => None,
    })
    .flatten()
}

/// Whether this frame's raw input of the running window carried an Escape press, whoever the runtime routed it to.
fn escape_arrived(ctx: &Context) -> bool {
    let viewport = ctx.viewport_id();
    ctx.with_plugin(|inputs: &mut WindowInputs| inputs.escapes.contains(&viewport))
        .unwrap_or(false)
}

/// First in the app's UI each pass: an Escape that no framework component took goes back into egui's input, where
/// DiscCraft's dialogs (`Modal::should_close`), drags and lists read it as before the runtime. The runtime keeps a bare
/// Escape out of egui while none of the app's own widgets has the focus, and offers it as the window's cancel; an
/// Escape while a press is held in a panel (a pick whip, a link or a clip being dragged) it takes as the cancel of
/// that press and offers to no one, and the app's own drag under the press takes it back.
pub(crate) fn give_back_escape(ctx: &Context) {
    // While a menu of the bar is open or the keyboard is on the bar, its Escape is the bar's: it reads the window's
    // cancel later in the pass, when it draws.
    if ctx.input(|input| input.key_pressed(egui::Key::Escape))
        || craft_menu::bar::is_open(ctx)
        || craft_menu::bar::is_focused(ctx)
    {
        return;
    }
    let cancel = with_window_intents(ctx, false, |intents| intents.take(Intent::Cancel));
    let held = escape_arrived(ctx) && ctx.input(|input| input.pointer.any_down());
    if !(cancel || held) {
        return;
    }
    // As egui's own Escape does at the start of a pass: a drag and drop (a pick whip) ends without dropping, and takes
    // the key.
    if egui::DragAndDrop::has_any_payload(ctx) {
        egui::DragAndDrop::clear_payload(ctx);
        return;
    }
    ctx.input_mut(|input| {
        input.events.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
    });
}

#[cfg(test)]
mod theme_tests {
    /// M9.54: DiscCraft's colours for the framework's parts read as a theme.
    #[test]
    fn the_theme_reads() {
        assert!(super::THEME.is_some(), "{}", super::theme_json());
    }
}

#[cfg(test)]
mod tests {
    use super::{before_input, give_back_escape, host_window, install_headless};
    use crate::actions::{Action, Trigger};
    use crate::i18n::Language;
    use egui::{Event, Key, Modifiers, RawInput};

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// One frame of `events` through the runtime, as the app runs it: what egui's input holds where the app's UI
    /// reads it (after an Escape no framework component took is handed back).
    fn frame(ctx: &egui::Context, events: Vec<Event>) -> Vec<Event> {
        let _ = before_input(ctx, Language::En);
        let mut seen = Vec::new();
        let input = RawInput {
            events,
            ..RawInput::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            host_window(ui);
            give_back_escape(ui.ctx());
            seen = ui.input(|input| input.events.clone());
        });
        // No renderer here: the font atlas upload is dropped on purpose.
        output.textures_delta.clear();
        seen
    }

    /// M9.54: with the UI runtime installed, the keys DiscCraft binds as Encore does still reach its shortcuts in
    /// egui's input: F6 (Layers), F10 (Progress), Ctrl+Q (Exit), the clipboard's events (Edit ▸ Cut, Copy, Paste),
    /// Delete, and an Escape (a dialog's, a drag's) that no framework component took.
    #[test]
    fn disccrafts_keys_reach_egui_with_the_runtime() {
        let ctx = egui::Context::default();
        install_headless(&ctx);
        let _ = frame(&ctx, Vec::new());
        let has = |events: &[Event], wanted: &Event| events.iter().any(|event| event == wanted);
        // Every key of DiscCraft's shortcuts, and the arrows and Space. (A bare Tab is the runtime's focus move among
        // the window's fields, as egui's own was; DiscCraft binds no Tab.)
        let shortcuts = Action::ALL
            .into_iter()
            .filter_map(|action| match action.trigger() {
                Some(Trigger::Key(shortcut)) => Some(key(shortcut.logical_key, shortcut.modifiers)),
                _ => None,
            });
        let more = [
            key(Key::ArrowLeft, Modifiers::NONE),
            key(Key::ArrowUp, Modifiers::SHIFT),
            key(Key::Space, Modifiers::NONE),
            key(Key::Enter, Modifiers::NONE),
            key(Key::Delete, Modifiers::SHIFT),
            Event::Copy,
            Event::Cut,
            Event::Paste("x".to_owned()),
        ];
        for event in shortcuts.chain(more) {
            let seen = frame(&ctx, vec![event.clone()]);
            assert!(has(&seen, &event), "{event:?} did not reach egui: {seen:?}");
            // The key goes up again, so the next press is no repeat.
            let release: Vec<Event> = match &event {
                Event::Key { key, modifiers, .. } => vec![Event::Key {
                    key: *key,
                    physical_key: Some(*key),
                    pressed: false,
                    repeat: false,
                    modifiers: *modifiers,
                }],
                _ => Vec::new(),
            };
            let _ = frame(&ctx, release);
        }
        let seen = frame(&ctx, vec![key(Key::Escape, Modifiers::NONE)]);
        let _ = &seen;
        assert!(
            seen.iter().any(|event| matches!(
                event,
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                }
            )),
            "Escape did not reach egui: {seen:?}"
        );
    }
}

#[cfg(test)]
mod escape_tests {
    use super::{before_input, give_back_escape, host_window, install_headless};
    use crate::i18n::Language;

    /// M9.54: Escape takes back a drag and drop (a pick whip being dragged) as egui's own Escape did, though the runtime
    /// keeps the key from egui: the payload goes, so the release drops nothing.
    #[test]
    fn escape_takes_back_a_drag_and_drop() {
        let ctx = egui::Context::default();
        install_headless(&ctx);
        let run = |events: Vec<egui::Event>| {
            let _ = before_input(&ctx, Language::En);
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..egui::RawInput::default()
                },
                |ui| {
                    host_window(ui);
                    give_back_escape(ui.ctx());
                },
            );
            output.textures_delta.clear();
        };
        run(Vec::new());
        egui::DragAndDrop::set_payload(&ctx, 7_u8);
        run(Vec::new());
        assert!(egui::DragAndDrop::has_any_payload(&ctx));
        run(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(!egui::DragAndDrop::has_any_payload(&ctx));
    }
}

#[cfg(test)]
mod app_tests {
    use super::install_headless;
    use crate::DiscCraftApp;
    use crate::workspace::Panel;

    /// M9.54: the whole window runs on the UI runtime: the menu bar and the panels' docking draw, every panel of the
    /// default workspace in front shows its body, and a menu row chosen comes back as a fired command.
    #[test]
    fn the_window_draws_its_panels_on_the_runtime() {
        let ctx = egui::Context::default();
        install_headless(&ctx);
        let mut app = DiscCraftApp::new(&ctx);
        // The default workspace, whatever this machine's settings hold.
        app.dock = crate::workspace::WorkspacePreset::Standard.dock();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            ..egui::RawInput::default()
        };
        for _ in 0..4 {
            app.framework_input(&ctx);
            let mut output = ctx.run_ui(input(), |ui| app.ui(ui));
            output.textures_delta.clear();
        }
        let drawn: Vec<Panel> = app.panel_bodies.iter().map(|(panel, _)| *panel).collect();
        for panel in [
            Panel::Project,
            Panel::Monitor,
            Panel::Properties,
            Panel::Library,
            Panel::TimelineViewer,
        ] {
            assert!(drawn.contains(&panel), "{panel:?} not drawn: {drawn:?}");
        }
        assert!(
            !app.menu_bar_model.menus.is_empty(),
            "the menu bar has its menus"
        );
    }
}

#[cfg(test)]
mod window_input_tests {
    use super::{WindowInputs, take_tab_menu};
    use crate::workspace::Panel;
    use egui::{Plugin as _, pos2};

    /// M9.55: a right click on a panel's tab, in whatever window, is DiscCraft's (its menu in the interface's
    /// language): the click and its release go from the input the UI runtime reads; elsewhere a right click stays.
    #[test]
    fn a_right_click_on_a_tab_is_disccrafts() {
        let ctx = egui::Context::default();
        let window = egui::ViewportId::from_hash_of("floated");
        let mut inputs = WindowInputs {
            tabs: vec![(
                window,
                Panel::Library,
                egui::Rect::from_min_size(pos2(10.0, 50.0), egui::vec2(80.0, 24.0)),
            )],
            ..WindowInputs::default()
        };
        let right = |at: egui::Pos2, pressed: bool| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut raw = egui::RawInput {
            viewport_id: window,
            events: vec![
                right(pos2(30.0, 60.0), true),
                right(pos2(30.0, 60.0), false),
            ],
            ..egui::RawInput::default()
        };
        inputs.input_hook(&ctx, &mut raw);
        assert!(raw.events.is_empty());
        assert_eq!(
            inputs.tab_menu,
            Some((window, Panel::Library, pos2(30.0, 60.0)))
        );
        // The same point in another window is no tab there.
        let mut raw = egui::RawInput {
            events: vec![right(pos2(30.0, 60.0), true)],
            ..egui::RawInput::default()
        };
        inputs.input_hook(&ctx, &mut raw);
        assert_eq!(raw.events.len(), 1);
        // Taken once, by its window.
        ctx.add_plugin(inputs);
        assert_eq!(take_tab_menu(&ctx, egui::ViewportId::ROOT), None);
        assert_eq!(
            take_tab_menu(&ctx, window),
            Some((Panel::Library, pos2(30.0, 60.0)))
        );
        assert_eq!(take_tab_menu(&ctx, window), None);
    }
}
