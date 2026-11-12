//! Menu commands and their default keyboard shortcuts.
//!
//! The key assignments follow the defaults of the classic disc authoring tool most DiscCraft users come from, so
//! that their hands already know them (docs/ui-parity.md §3). `COMMAND` is Ctrl on Windows/Linux and Cmd on macOS.

use crate::i18n::tl;
use egui::{AtomExt as _, Context, Event, Key, KeyboardShortcut, Modifiers, Ui};
use std::cmp::Ordering;

/// A command reachable from the menu bar and, for most, from the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Action {
    /// Create a new project.
    NewProject,
    /// Create a new menu.
    NewMenu,
    /// Create a new timeline.
    NewTimeline,
    /// Create a new slideshow.
    NewSlideshow,
    /// Create a new playlist.
    NewPlaylist,
    /// Create a new chapter playlist of the selected timeline.
    NewChapterPlaylist,
    /// Turn the selected text, image or shape into a button (Object ▸ Convert to Button).
    ConvertToButton,
    /// Turn the selected buttons back into plain objects (Object ▸ Convert to Object).
    ConvertToObject,
    /// Open the Specify Link dialog for the selected buttons or the selected object's end action.
    SpecifyLink,
    /// Open the Drop Shadow dialog for the selected text, graphic or buttons (Object ▸ Drop Shadow…).
    DropShadow,
    /// Mirror the selection left to right (Object ▸ Flip Horizontal).
    FlipHorizontal,
    /// Mirror the selection top to bottom (Object ▸ Flip Vertical).
    FlipVertical,
    /// Turn the selection half a turn (Object ▸ Rotate ▸ 180°).
    Rotate180,
    /// Turn the selection a quarter turn clockwise (Object ▸ Rotate ▸ 90° (Clockwise)).
    RotateClockwise,
    /// Turn the selection a quarter turn counter-clockwise (Object ▸ Rotate ▸ 90° (Counterclockwise)).
    RotateCounterClockwise,
    /// Open an existing project.
    OpenProject,
    /// Close the project and return to the start screen.
    CloseProject,
    /// Save the current project.
    Save,
    /// Save the current project under a new name.
    SaveAs,
    /// Import a file as an asset.
    ImportAsset,
    /// Edit the authoring mode, aspect ratio and capacity of the open project.
    ProjectSettings,
    /// Make the selected timeline, menu, slideshow or playlist the disc's First Play.
    SetFirstPlay,
    /// Set the remote operations prohibited on the whole disc.
    DiscUserOps,
    /// Check the current project for problems.
    CheckProject,
    /// Preview the current project.
    Preview,
    /// Build a folder.
    BuildFolder,
    /// Build an image.
    BuildImage,
    /// Exit the application.
    Exit,
    /// Undo the last change.
    Undo,
    /// Redo the last undone change.
    Redo,
    /// Rename the selected item.
    Rename,
    /// Open the Preferences dialog.
    Preferences,
    /// Cut the selected buttons.
    Cut,
    /// Copy the selected buttons.
    Copy,
    /// Paste buttons into the open menu.
    Paste,
    /// Duplicate the selected buttons.
    Duplicate,
    /// Clear the selected item.
    Clear,
    /// Select every button of the open menu.
    SelectAll,
    /// Deselect every button.
    DeselectAll,
    /// Edit the menu in PhotoCraft (Encore: Edit Menu in Photoshop).
    EditMenuInPhotoCraft,
    /// Edit the selected asset in the app that makes it (Encore: Edit Original).
    EditOriginal,
    /// Make an EffectCraft composition of the menu (Encore: Create After Effects Composition).
    CreateEffectCraftComposition,
    /// Open the Menu Color Set dialog for the menu being edited (Menu ▸ Edit Menu Color Set…).
    EditMenuColorSet,
    /// The User Operations dialog of the menu being edited (Menu ▸ Set Menu User Operations…).
    SetMenuUserOps,
    /// Save the menu being edited as a layered PSD (Menu ▸ Save Menu as File…).
    SaveMenuAsFile,
    /// Save the menu being edited into the library's User set (Menu ▸ Save Menu as Template…).
    SaveMenuAsTemplate,
    /// The Specify Link dialog of the menu's end action (Menu ▸ Set End Action…).
    SetMenuEnd,
    /// Align the left edges of the selected buttons.
    AlignLeft,
    /// Align the horizontal centres.
    AlignHorizontalCenter,
    /// Align the right edges.
    AlignRight,
    /// Align the top edges.
    AlignTop,
    /// Align the vertical centres.
    AlignVerticalCenter,
    /// Align the bottom edges.
    AlignBottom,
    /// Toggle aligning to the safe area instead of the selection.
    AlignToSafeArea,
    /// Bring the selected buttons to the front (the last numbers).
    BringToFront,
    /// Bring the selected buttons one step forward.
    BringForward,
    /// Send the selected buttons one step backward.
    SendBackward,
    /// Send the selected buttons to the back (the first numbers).
    SendToBack,
    /// Spread the selected buttons evenly from top to bottom.
    DistributeVertically,
    /// Spread the selected buttons evenly from left to right.
    DistributeHorizontally,
    /// Toggle distributing within the safe area instead of the selection.
    DistributeToSafeArea,
    /// Add a button.
    AddButton,
    /// Create a chapter index menu for the selected timeline.
    CreateChapterIndex,
    /// Create a chapter index spread over several menus.
    CreateMultiPageChapterIndex,
    /// Add a chapter point.
    AddChapterPoint,
    /// Add chapter points at a fixed interval.
    AddChapterPointsAtInterval,
    /// Play or stop the timeline in the Timeline Viewer.
    Play,
    /// Add an audio track to the selected timeline.
    AddAudioTrack,
    /// Add a subtitle track to the selected timeline.
    AddSubtitleTrack,
    /// Delete the bottom audio track of the selected timeline.
    RemoveAudioTrack,
    /// Delete the bottom subtitle track of the selected timeline.
    RemoveSubtitleTrack,
    /// Read a subtitle file (SRT, WebVTT, text script) and add it as a subtitle track.
    ImportSubtitles,
    /// Show the About DiscCraft dialog.
    About,
    /// Zoom in.
    ZoomIn,
    /// Zoom out.
    ZoomOut,
    /// Fit the current view in the window.
    FitInWindow,
    /// Show the normal subpicture.
    ShowNormal,
    /// Show the selected subpicture.
    ShowSelected,
    /// Show the activated subpicture.
    ShowActivated,
    /// Show safe areas.
    ShowSafeAreas,
    /// Show button routing.
    ShowRouting,
    /// Show the menu guides.
    ShowGuides,
    /// Snap dragged buttons to the guides.
    SnapToGuides,
    /// Keep the guides from being dragged.
    LockGuides,
    /// Remove every guide of the menu.
    ClearGuides,
    /// Add a guide at a typed position.
    NewGuide,
    /// Show thumbnails on the flowchart's nodes.
    ShowAllThumbnails,
    /// Show chapter names next to the chapter points on the timeline.
    ShowChapterNames,
    /// Show or hide the Build window.
    WindowBuild,
    /// Show or hide the Check Project window.
    WindowCheck,
    /// Show or hide the Disc window.
    WindowDisc,
    /// Show or hide the Flowchart window.
    WindowFlowchart,
    /// Show or hide the Layers panel.
    WindowLayers,
    /// Show or hide the Library panel.
    WindowLibrary,
    /// Show or hide the Menus window.
    WindowMenus,
    /// Show or hide the Monitor window.
    WindowMonitor,
    /// Show or hide the Project window.
    WindowProject,
    /// Show or hide the Properties window.
    WindowProperties,
    /// Show or hide the Timelines window.
    WindowTimelines,
    /// Bring the Menu Viewer forward.
    WindowMenuViewer,
    /// Show or hide the Slideshow Viewer.
    WindowSlideshowViewer,
    /// Show or hide the Timeline Viewer.
    WindowTimelineViewer,
}

/// What triggers an action from the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trigger {
    /// A key with modifiers.
    Key(KeyboardShortcut),
    /// A typed character (for keys egui has no `Key` for, such as `*` on the numeric keypad).
    Text(&'static str),
}

impl Action {
    /// Every action, in menu order.
    pub(crate) const ALL: [Action; 102] = [
        Action::NewProject,
        Action::NewMenu,
        Action::NewTimeline,
        Action::NewSlideshow,
        Action::NewPlaylist,
        Action::NewChapterPlaylist,
        Action::ConvertToButton,
        Action::ConvertToObject,
        Action::SpecifyLink,
        Action::DropShadow,
        Action::FlipHorizontal,
        Action::FlipVertical,
        Action::Rotate180,
        Action::RotateClockwise,
        Action::RotateCounterClockwise,
        Action::OpenProject,
        Action::CloseProject,
        Action::Save,
        Action::SaveAs,
        Action::ImportAsset,
        Action::ProjectSettings,
        Action::SetFirstPlay,
        Action::DiscUserOps,
        Action::CheckProject,
        Action::Preview,
        Action::BuildFolder,
        Action::BuildImage,
        Action::Exit,
        Action::Undo,
        Action::Redo,
        Action::Rename,
        Action::Preferences,
        Action::Cut,
        Action::Copy,
        Action::Paste,
        Action::Duplicate,
        Action::Clear,
        Action::SelectAll,
        Action::DeselectAll,
        Action::EditMenuInPhotoCraft,
        Action::EditOriginal,
        Action::CreateEffectCraftComposition,
        Action::EditMenuColorSet,
        Action::SetMenuUserOps,
        Action::SaveMenuAsFile,
        Action::SaveMenuAsTemplate,
        Action::SetMenuEnd,
        Action::AlignLeft,
        Action::AlignHorizontalCenter,
        Action::AlignRight,
        Action::AlignTop,
        Action::AlignVerticalCenter,
        Action::AlignBottom,
        Action::AlignToSafeArea,
        Action::BringToFront,
        Action::BringForward,
        Action::SendBackward,
        Action::SendToBack,
        Action::DistributeVertically,
        Action::DistributeHorizontally,
        Action::DistributeToSafeArea,
        Action::AddButton,
        Action::CreateChapterIndex,
        Action::CreateMultiPageChapterIndex,
        Action::AddChapterPoint,
        Action::AddChapterPointsAtInterval,
        Action::Play,
        Action::AddAudioTrack,
        Action::AddSubtitleTrack,
        Action::RemoveAudioTrack,
        Action::RemoveSubtitleTrack,
        Action::ImportSubtitles,
        Action::About,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::FitInWindow,
        Action::ShowNormal,
        Action::ShowSelected,
        Action::ShowActivated,
        Action::ShowSafeAreas,
        Action::ShowRouting,
        Action::ShowGuides,
        Action::SnapToGuides,
        Action::LockGuides,
        Action::ClearGuides,
        Action::NewGuide,
        Action::ShowAllThumbnails,
        Action::ShowChapterNames,
        Action::WindowBuild,
        Action::WindowCheck,
        Action::WindowDisc,
        Action::WindowFlowchart,
        Action::WindowLayers,
        Action::WindowLibrary,
        Action::WindowMenus,
        Action::WindowMonitor,
        Action::WindowProject,
        Action::WindowProperties,
        Action::WindowTimelines,
        Action::WindowMenuViewer,
        Action::WindowSlideshowViewer,
        Action::WindowTimelineViewer,
    ];

    /// Menu label (English source, translated by the catalog).
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::NewProject | Self::WindowProject => tl!("Project"),
            Self::NewMenu => tl!("Menu"),
            Self::NewTimeline => tl!("Timeline"),
            Self::NewSlideshow => tl!("Slideshow"),
            Self::NewPlaylist => tl!("Playlist"),
            Self::NewChapterPlaylist => tl!("Chapter Playlist"),
            Self::ConvertToButton => tl!("Convert to Button"),
            Self::ConvertToObject => tl!("Convert to Object"),
            Self::SpecifyLink => tl!("Specify Link…"),
            Self::DropShadow => tl!("Drop Shadow…"),
            Self::FlipHorizontal => tl!("Flip Horizontal"),
            Self::FlipVertical => tl!("Flip Vertical"),
            Self::Rotate180 => tl!("180°"),
            Self::RotateClockwise => tl!("90° (Clockwise)"),
            Self::RotateCounterClockwise => tl!("90° (Counterclockwise)"),
            Self::OpenProject => tl!("Open Project…"),
            Self::CloseProject => tl!("Close Project"),
            Self::ProjectSettings => tl!("Project Settings…"),
            Self::SetFirstPlay => tl!("Set as First Play"),
            Self::DiscUserOps => tl!("Set Disc User Operations…"),
            Self::Save => tl!("Save"),
            Self::SaveAs => tl!("Save As…"),
            Self::ImportAsset => tl!("Import as Asset…"),
            Self::CheckProject => tl!("Check Project…"),
            Self::Preview => tl!("Preview"),
            Self::BuildFolder => tl!("Folder…"),
            Self::BuildImage => tl!("Image…"),
            Self::Exit => tl!("Exit"),
            Self::Undo => tl!("Undo"),
            Self::Redo => tl!("Redo"),
            Self::Rename => tl!("Rename…"),
            Self::Preferences => tl!("Preferences…"),
            Self::Cut => tl!("Cut"),
            Self::Copy => tl!("Copy"),
            Self::Paste => tl!("Paste"),
            Self::Duplicate => tl!("Duplicate"),
            Self::Clear => tl!("Clear"),
            Self::SelectAll => tl!("Select All"),
            Self::DeselectAll => tl!("Deselect All"),
            Self::EditMenuInPhotoCraft => tl!("Edit Menu in PhotoCraft"),
            Self::EditOriginal => tl!("Edit Original"),
            Self::CreateEffectCraftComposition => tl!("Create EffectCraft Composition…"),
            Self::EditMenuColorSet => tl!("Edit Menu Color Set…"),
            Self::SetMenuUserOps => tl!("Set Menu User Operations…"),
            Self::SaveMenuAsFile => tl!("Save Menu as File…"),
            Self::SaveMenuAsTemplate => tl!("Save Menu as Template…"),
            Self::SetMenuEnd => tl!("Set End Action…"),
            Self::AlignLeft => tl!("Left Edges"),
            Self::AlignHorizontalCenter => tl!("Horizontal Centers"),
            Self::AlignRight => tl!("Right Edges"),
            Self::AlignTop => tl!("Top Edges"),
            Self::AlignVerticalCenter => tl!("Vertical Centers"),
            Self::AlignBottom => tl!("Bottom Edges"),
            Self::AlignToSafeArea => tl!("Align Relative to Safe Area"),
            Self::BringToFront => tl!("Bring to Front"),
            Self::BringForward => tl!("Bring Forward"),
            Self::SendBackward => tl!("Send Backward"),
            Self::SendToBack => tl!("Send to Back"),
            Self::DistributeVertically => tl!("Vertically"),
            Self::DistributeHorizontally => tl!("Horizontally"),
            Self::DistributeToSafeArea => tl!("Distribute Relative to Safe Area"),
            Self::AddButton => tl!("Add Button"),
            Self::CreateChapterIndex => tl!("Create Chapter Index"),
            Self::CreateMultiPageChapterIndex => tl!("Create Multi-Page Chapter Index"),
            Self::AddChapterPoint => tl!("Add Chapter Point"),
            Self::AddChapterPointsAtInterval => tl!("Add Chapter Points at Interval…"),
            Self::Play => tl!("Play"),
            Self::AddAudioTrack => tl!("Add Audio Track…"),
            Self::AddSubtitleTrack => tl!("Add Subtitle Track…"),
            Self::RemoveAudioTrack => tl!("Delete Audio Track"),
            Self::RemoveSubtitleTrack => tl!("Delete Subtitle Track"),
            Self::ImportSubtitles => tl!("Import Subtitles…"),
            Self::About => tl!("About DiscCraft…"),
            Self::ZoomIn => tl!("Zoom In"),
            Self::ZoomOut => tl!("Zoom Out"),
            Self::FitInWindow => tl!("Fit in Window"),
            Self::ShowNormal => tl!("Show Normal Subpicture"),
            Self::ShowSelected => tl!("Show Selected Subpicture"),
            Self::ShowActivated => tl!("Show Activated Subpicture"),
            Self::ShowSafeAreas => tl!("Show Safe Areas"),
            Self::ShowRouting => tl!("Show Button Routing"),
            Self::ShowGuides => tl!("Show Guides"),
            Self::SnapToGuides => tl!("Snap to Guides"),
            Self::LockGuides => tl!("Lock Guides"),
            Self::ClearGuides => tl!("Clear Guides"),
            Self::NewGuide => tl!("New Guide…"),
            Self::ShowAllThumbnails => tl!("Show All Thumbnails"),
            Self::ShowChapterNames => tl!("Show Chapter Names"),
            Self::WindowBuild => tl!("Build"),
            Self::WindowCheck => tl!("Check Project"),
            Self::WindowDisc => tl!("Disc"),
            Self::WindowFlowchart => tl!("Flowchart"),
            Self::WindowMenus => tl!("Menus"),
            Self::WindowMonitor => tl!("Monitor"),
            Self::WindowProperties => tl!("Properties"),
            Self::WindowTimelines => tl!("Timelines"),
            Self::WindowLayers => tl!("Layers"),
            Self::WindowLibrary => tl!("Library"),
            Self::WindowMenuViewer => tl!("Menu Viewer"),
            Self::WindowSlideshowViewer => tl!("Slideshow Viewer"),
            Self::WindowTimelineViewer => tl!("Timeline Viewer"),
        }
    }

    /// The default keyboard trigger, if any.
    pub(crate) fn trigger(self) -> Option<Trigger> {
        match self {
            Self::NewProject => key_shortcut(Modifiers::COMMAND, Key::N),
            Self::NewMenu => key_shortcut(Modifiers::COMMAND, Key::M),
            Self::NewTimeline => key_shortcut(Modifiers::COMMAND, Key::T),
            Self::NewSlideshow => key_shortcut(Modifiers::COMMAND, Key::G),
            Self::NewPlaylist
            | Self::NewChapterPlaylist
            | Self::ConvertToObject
            | Self::BuildFolder
            | Self::BuildImage
            | Self::AddButton
            | Self::CreateChapterIndex
            | Self::CreateMultiPageChapterIndex
            | Self::AddChapterPointsAtInterval
            | Self::AddAudioTrack
            | Self::AddSubtitleTrack
            | Self::RemoveAudioTrack
            | Self::RemoveSubtitleTrack
            | Self::ImportSubtitles
            | Self::About
            | Self::AlignLeft
            | Self::AlignHorizontalCenter
            | Self::AlignRight
            | Self::AlignTop
            | Self::AlignVerticalCenter
            | Self::AlignBottom
            | Self::AlignToSafeArea
            | Self::DistributeVertically
            | Self::DistributeHorizontally
            | Self::DistributeToSafeArea
            | Self::WindowDisc
            | Self::WindowMenuViewer
            | Self::WindowSlideshowViewer
            | Self::WindowTimelineViewer => None,
            Self::OpenProject => key_shortcut(Modifiers::COMMAND, Key::O),
            Self::CloseProject => key_shortcut(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::W),
            Self::ProjectSettings | Self::SetFirstPlay => None,
            Self::Save => key_shortcut(Modifiers::COMMAND, Key::S),
            Self::SaveAs => key_shortcut(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S),
            Self::ImportAsset => key_shortcut(Modifiers::COMMAND, Key::I),
            Self::CheckProject => key_shortcut(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::L),
            Self::Preview => key_shortcut(Modifiers::COMMAND.plus(Modifiers::ALT), Key::Space),
            Self::Exit => key_shortcut(Modifiers::COMMAND, Key::Q),
            Self::Undo => key_shortcut(Modifiers::COMMAND, Key::Z),
            Self::Redo => key_shortcut(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z),
            Self::Rename => key_shortcut(Modifiers::COMMAND.plus(Modifiers::ALT), Key::R),
            Self::Preferences => key_shortcut(Modifiers::COMMAND, Key::K),
            Self::Cut => key_shortcut(Modifiers::COMMAND, Key::X),
            Self::Copy => key_shortcut(Modifiers::COMMAND, Key::C),
            Self::Paste => key_shortcut(Modifiers::COMMAND, Key::V),
            Self::Duplicate => key_shortcut(Modifiers::COMMAND, Key::D),
            Self::Clear => key_shortcut(Modifiers::NONE, Key::Delete),
            Self::SelectAll => key_shortcut(Modifiers::COMMAND, Key::A),
            Self::BringToFront => {
                key_shortcut(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::CloseBracket)
            }
            Self::BringForward => key_shortcut(Modifiers::COMMAND, Key::CloseBracket),
            Self::SendBackward => key_shortcut(Modifiers::COMMAND, Key::OpenBracket),
            Self::SendToBack => {
                key_shortcut(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::OpenBracket)
            }
            Self::DeselectAll => key_shortcut(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::A),
            Self::EditMenuInPhotoCraft => {
                key_shortcut(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::M)
            }
            Self::EditOriginal => key_shortcut(Modifiers::COMMAND, Key::E),
            Self::ConvertToButton => key_shortcut(Modifiers::COMMAND, Key::B),
            Self::SpecifyLink => key_shortcut(Modifiers::COMMAND, Key::L),
            Self::DropShadow => key_shortcut(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::O),
            Self::FlipHorizontal => key_shortcut(Modifiers::COMMAND.plus(Modifiers::ALT), Key::H),
            Self::FlipVertical => key_shortcut(Modifiers::COMMAND.plus(Modifiers::ALT), Key::V),
            Self::Rotate180 | Self::RotateClockwise | Self::RotateCounterClockwise => None,
            Self::CreateEffectCraftComposition => None,
            Self::EditMenuColorSet => None,
            Self::SaveMenuAsTemplate => {
                key_shortcut(Modifiers::COMMAND.plus(Modifiers::ALT), Key::M)
            }
            Self::SetMenuUserOps | Self::SaveMenuAsFile | Self::SetMenuEnd => None,
            Self::DiscUserOps => None,
            Self::AddChapterPoint => Some(Trigger::Text("*")),
            Self::Play => key_shortcut(Modifiers::NONE, Key::Space),
            Self::ZoomIn => key_shortcut(Modifiers::NONE, Key::Plus),
            Self::ZoomOut => key_shortcut(Modifiers::NONE, Key::Minus),
            Self::FitInWindow => key_shortcut(Modifiers::NONE, Key::Backslash),
            Self::ShowNormal => key_shortcut(Modifiers::COMMAND, Key::Num3),
            Self::ShowSelected => key_shortcut(Modifiers::COMMAND, Key::Num4),
            Self::ShowActivated => key_shortcut(Modifiers::COMMAND, Key::Num5),
            Self::ShowSafeAreas => key_shortcut(Modifiers::COMMAND, Key::Num6),
            Self::ShowRouting => key_shortcut(Modifiers::COMMAND, Key::Num7),
            // Encore: Ctrl+: / Shift+Ctrl+: / Alt+Ctrl+: / Shift+Alt+Ctrl+:.
            Self::ShowGuides => key_shortcut(Modifiers::COMMAND, Key::Colon),
            Self::SnapToGuides => {
                key_shortcut(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Colon)
            }
            Self::LockGuides => key_shortcut(Modifiers::COMMAND.plus(Modifiers::ALT), Key::Colon),
            Self::ClearGuides => None,
            Self::ShowAllThumbnails => None,
            Self::NewGuide => key_shortcut(
                Modifiers::COMMAND
                    .plus(Modifiers::SHIFT)
                    .plus(Modifiers::ALT),
                Key::Colon,
            ),
            Self::ShowChapterNames => key_shortcut(Modifiers::COMMAND, Key::Num8),
            Self::WindowBuild => key_shortcut(Modifiers::NONE, Key::F2),
            Self::WindowCheck => key_shortcut(Modifiers::NONE, Key::F4),
            Self::WindowFlowchart => key_shortcut(Modifiers::NONE, Key::F5),
            Self::WindowLayers => key_shortcut(Modifiers::NONE, Key::F6),
            Self::WindowLibrary => key_shortcut(Modifiers::NONE, Key::F7),
            Self::WindowMenus => key_shortcut(Modifiers::NONE, Key::F8),
            Self::WindowMonitor => key_shortcut(Modifiers::NONE, Key::F9),
            Self::WindowProject => key_shortcut(Modifiers::NONE, Key::F11),
            Self::WindowProperties => key_shortcut(Modifiers::NONE, Key::F12),
            Self::WindowTimelines => key_shortcut(Modifiers::COMMAND, Key::F3),
        }
    }

    /// Whether the shortcut may fire while a text field has keyboard focus. True only for `Trigger::Key` with
    /// COMMAND or ALT held, or a function key (F1–F12) — except the editing keys text fields have their own
    /// meaning for (Undo, Redo, Cut, Copy, Paste, Duplicate, Select All, Deselect All).
    pub(crate) fn while_typing(self) -> bool {
        match self.trigger() {
            Some(Trigger::Key(shortcut)) => while_typing_key(self, shortcut),
            Some(Trigger::Text(_)) | None => false,
        }
    }

    /// Shortcut text for the menu (`ctx.format_shortcut` for keys, the character itself for `Text`), "" when none.
    pub(crate) fn shortcut_text(self, ctx: &Context) -> String {
        match self.trigger() {
            // egui names the bracket keys; show the characters, as on the key caps.
            Some(Trigger::Key(shortcut)) => ctx
                .format_shortcut(&shortcut)
                .replace("CloseBracket", "]")
                .replace("OpenBracket", "[")
                .replace("Backslash", "\\")
                .replace("Colon", ":")
                .replace("Plus", "+")
                .replace("Minus", "-"),
            Some(Trigger::Text(text)) => text.to_owned(),
            None => String::new(),
        }
    }
}

/// Builds a key trigger from modifiers and a logical key.
fn key_shortcut(modifiers: Modifiers, key: Key) -> Option<Trigger> {
    Some(Trigger::Key(KeyboardShortcut::new(modifiers, key)))
}

/// Whether a key trigger may fire while typing, except for the text editing keys.
fn while_typing_key(action: Action, shortcut: KeyboardShortcut) -> bool {
    if matches!(
        action,
        Action::Undo
            | Action::Redo
            | Action::Cut
            | Action::Copy
            | Action::Paste
            | Action::Duplicate
            | Action::SelectAll
            | Action::DeselectAll
    ) {
        false
    } else {
        shortcut.modifiers.command
            || shortcut.modifiers.alt
            || is_function_key(shortcut.logical_key)
    }
}

/// Whether a key is one of the function keys F1 through F12.
fn is_function_key(key: Key) -> bool {
    matches!(
        key,
        Key::F1
            | Key::F2
            | Key::F3
            | Key::F4
            | Key::F5
            | Key::F6
            | Key::F7
            | Key::F8
            | Key::F9
            | Key::F10
            | Key::F11
            | Key::F12
    )
}

/// Number of relevant modifiers in a trigger; text triggers count as zero.
fn modifier_count(trigger: Trigger) -> usize {
    match trigger {
        Trigger::Key(shortcut) => {
            usize::from(shortcut.modifiers.command)
                + usize::from(shortcut.modifiers.shift)
                + usize::from(shortcut.modifiers.alt)
        }
        Trigger::Text(_) => 0,
    }
}

/// Actions with a trigger, most specific first: more modifiers before fewer (so Shift+Ctrl+S is tested before
/// Ctrl+S), `Text` triggers last. Stable for equal counts (keeps `ALL` order). Pure; unit-tested.
pub(crate) fn match_order() -> Vec<Action> {
    let mut actions: Vec<Action> = Action::ALL
        .iter()
        .copied()
        .filter(|action| action.trigger().is_some())
        .collect();

    actions.sort_by(|left, right| match (left.trigger(), right.trigger()) {
        (Some(Trigger::Key(_)), Some(Trigger::Text(_))) => Ordering::Less,
        (Some(Trigger::Text(_)), Some(Trigger::Key(_))) => Ordering::Greater,
        (Some(left_trigger), Some(right_trigger)) => {
            modifier_count(right_trigger).cmp(&modifier_count(left_trigger))
        }
        (None, None) | (Some(_), None) | (None, Some(_)) => Ordering::Equal,
    });

    actions
}

/// The actions pressed this frame, in `match_order`. Consumes the matching key events so widgets do not also see
/// them. When `ctx.egui_wants_keyboard_input()`, only actions with `while_typing()` are considered (and `Text`
/// events are left alone). A `Text` trigger matches an `Event::Text(t)` with `t == trigger` (remove that event).
pub(crate) fn pressed_actions(ctx: &Context) -> Vec<Action> {
    let wants_keyboard = ctx.egui_wants_keyboard_input();
    let mut pressed = Vec::new();

    for action in match_order() {
        let Some(trigger) = action.trigger() else {
            continue;
        };

        if wants_keyboard && !action.while_typing() {
            continue;
        }

        match trigger {
            // The window turns Ctrl+X / C / V into clipboard events instead of key presses.
            Trigger::Key(_) if is_clipboard(action) => {
                if !wants_keyboard && take_clipboard_event(ctx, action) {
                    pressed.push(action);
                }
            }
            Trigger::Key(shortcut) => {
                let alternate = shifted_alternate(shortcut);
                if ctx.input_mut(|input| {
                    input.consume_shortcut(&shortcut)
                        || alternate.is_some_and(|other| input.consume_shortcut(&other))
                }) {
                    pressed.push(action);
                }
            }
            Trigger::Text(text) => {
                if !wants_keyboard {
                    let mut found = false;
                    ctx.input_mut(|input| {
                        input.events.retain(|event| {
                            if found {
                                true
                            } else if let Event::Text(event_text) = event {
                                if event_text.as_str() == text {
                                    found = true;
                                    false
                                } else {
                                    true
                                }
                            } else {
                                true
                            }
                        });
                    });

                    if found {
                        pressed.push(action);
                    }
                }
            }
        }
    }

    pressed
}

/// With Shift held, the bracket keys type `{` and `}`, and the window reports those characters as the key. The
/// same shortcut with the curly bracket, so Shift+Ctrl+] works on every keyboard layout.
fn shifted_alternate(shortcut: KeyboardShortcut) -> Option<KeyboardShortcut> {
    if !shortcut.modifiers.shift {
        return None;
    }
    let key = match shortcut.logical_key {
        Key::OpenBracket => Key::OpenCurlyBracket,
        Key::CloseBracket => Key::CloseCurlyBracket,
        _ => return None,
    };
    Some(KeyboardShortcut::new(shortcut.modifiers, key))
}

/// Cut, Copy and Paste, which arrive as clipboard events rather than key presses.
fn is_clipboard(action: Action) -> bool {
    matches!(action, Action::Cut | Action::Copy | Action::Paste)
}

/// Removes this frame's clipboard event for `action`; returns whether there was one.
fn take_clipboard_event(ctx: &Context, action: Action) -> bool {
    let wanted = |event: &Event| match action {
        Action::Cut => matches!(event, Event::Cut),
        Action::Copy => matches!(event, Event::Copy),
        Action::Paste => matches!(event, Event::Paste(_)),
        _ => false,
    };
    ctx.input_mut(|input| {
        let mut found = false;
        input.events.retain(|event| {
            if !found && wanted(event) {
                found = true;
                false
            } else {
                true
            }
        });
        found
    })
}

/// The text of this frame's paste event (Ctrl+V), if any, without consuming it. Read before
/// [`pressed_actions`], which consumes the event when no text field has focus.
pub(crate) fn pasted_text(ctx: &Context) -> Option<String> {
    ctx.input(|input| {
        input.events.iter().find_map(|event| match event {
            Event::Paste(text) => Some(text.clone()),
            _ => None,
        })
    })
}

/// A menu item for `action` with its shortcut text; returns whether it was clicked.
pub(crate) fn menu_item(ui: &mut Ui, action: Action, enabled: bool) -> bool {
    let button = egui::Button::new(action.label()).shortcut_text(action.shortcut_text(ui.ctx()));
    ui.add_enabled(enabled, button).clicked()
}

/// Width of the check mark column in checkable menu items.
const CHECK_WIDTH: f32 = 16.0;

/// A checkable menu item (panel visibility, overlays); returns whether it was clicked. Like Encore, a checked item
/// shows a check mark in a column on the left instead of a highlighted background, so the open panels read at a
/// glance and the labels stay aligned.
pub(crate) fn menu_check(ui: &mut Ui, action: Action, checked: bool) -> bool {
    let mark = if checked { "✔" } else { "" };
    let button = egui::Button::new((mark.atom_size(egui::vec2(CHECK_WIDTH, 0.0)), action.label()))
        .shortcut_text(action.shortcut_text(ui.ctx()));
    ui.add(button).clicked()
}

#[cfg(test)]
mod tests {
    use super::{Action, Trigger, match_order};
    use egui::{Key, KeyboardShortcut, Modifiers};

    /// Position of an action in an order, if present.
    fn position(order: &[Action], action: Action) -> Option<usize> {
        order.iter().position(|candidate| *candidate == action)
    }

    /// Whether an action has a COMMAND-only key trigger.
    fn is_command_only(action: Action) -> bool {
        match action.trigger() {
            Some(Trigger::Key(shortcut)) => {
                shortcut.modifiers.command && !shortcut.modifiers.shift && !shortcut.modifiers.alt
            }
            Some(Trigger::Text(_)) | None => false,
        }
    }

    /// No two actions share a trigger.
    #[test]
    fn triggers_are_unique() {
        for (i, left) in Action::ALL.iter().enumerate() {
            for right in Action::ALL.iter().skip(i + 1) {
                let (left, right) = (left.trigger(), right.trigger());
                if left.is_some() && right.is_some() {
                    assert_ne!(left, right);
                }
            }
        }
    }

    /// Match order is most specific first and contains exactly triggered actions.
    #[test]
    fn match_order_is_specific_first() {
        let order = match_order();

        assert!(matches!(
            (position(&order, Action::SaveAs), position(&order, Action::Save)),
            (Some(save_as), Some(save)) if save_as < save
        ));

        assert!(matches!(
            (position(&order, Action::Redo), position(&order, Action::Undo)),
            (Some(redo), Some(undo)) if redo < undo
        ));

        let check = position(&order, Action::CheckProject);
        assert!(check.is_some());

        for action in &order {
            if *action == Action::CheckProject {
                continue;
            }
            if is_command_only(*action) {
                assert!(matches!(
                    (check, position(&order, *action)),
                    (Some(check_pos), Some(other_pos)) if check_pos < other_pos
                ));
            }
        }

        assert_eq!(order.last(), Some(&Action::AddChapterPoint));

        let triggered_count = Action::ALL
            .iter()
            .filter(|action| action.trigger().is_some())
            .count();
        assert_eq!(order.len(), triggered_count);
        assert!(order.iter().all(|action| action.trigger().is_some()));
    }

    /// While-typing behavior matches the documented exceptions.
    #[test]
    fn while_typing_rules() {
        assert!(Action::Save.while_typing());
        assert!(Action::SaveAs.while_typing());
        assert!(Action::NewMenu.while_typing());
        assert!(Action::WindowFlowchart.while_typing());

        assert!(!Action::Undo.while_typing());
        assert!(!Action::Redo.while_typing());
        assert!(!Action::Clear.while_typing());
        assert!(!Action::ZoomIn.while_typing());
        assert!(!Action::AddChapterPoint.while_typing());
    }

    /// Spot-checks selected default triggers.
    #[test]
    fn default_triggers_spot_check() {
        assert_eq!(
            Action::NewMenu.trigger(),
            Some(Trigger::Key(KeyboardShortcut::new(
                Modifiers::COMMAND,
                Key::M
            )))
        );

        assert_eq!(
            Action::Preview.trigger(),
            Some(Trigger::Key(KeyboardShortcut::new(
                Modifiers::COMMAND.plus(Modifiers::ALT),
                Key::Space
            )))
        );

        assert_eq!(
            Action::WindowTimelines.trigger(),
            Some(Trigger::Key(KeyboardShortcut::new(
                Modifiers::COMMAND,
                Key::F3
            )))
        );

        assert_eq!(Action::AddChapterPoint.trigger(), Some(Trigger::Text("*")));

        assert_eq!(Action::NewPlaylist.trigger(), None);
    }

    /// Shift+Ctrl+] also matches the `}` the keyboard types with Shift held.
    #[test]
    fn shifted_brackets_have_a_curly_alternate() {
        let front =
            KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::CloseBracket);
        assert_eq!(
            super::shifted_alternate(front).map(|shortcut| shortcut.logical_key),
            Some(Key::CloseCurlyBracket)
        );
        let forward = KeyboardShortcut::new(Modifiers::COMMAND, Key::CloseBracket);
        assert_eq!(super::shifted_alternate(forward), None);
    }

    /// Every label is non-empty.
    #[test]
    fn labels_are_non_empty() {
        for action in Action::ALL {
            assert!(!action.label().is_empty());
        }
    }
}
