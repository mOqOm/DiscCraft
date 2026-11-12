//! The "Specify Link" dialog (Encore's リンクの指定…, Ctrl+L): the project's targets as a grouped list, the chosen
//! link, OK / Cancel. It sets the selected buttons' links, or the selected object's end action.

use crate::app::{DiscCraftApp, Selection};
use crate::flowchart::{NodeId, Port, command_for_port};
use crate::i18n::{tl, tlf};
use crate::menu_editor::{link_label, link_targets};
use disccraft_project::{Link, MenuId};
use egui::{Context, Id, Modal, Ui};
use serde_json::json;

/// What the dialog sets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LinkTarget {
    /// The links of these buttons of a menu.
    Buttons(MenuId, Vec<usize>),
    /// The end action of a node (timeline, slideshow, playlist, chapter playlist or menu).
    EndAction(NodeId),
    /// Any one link field (its ▶ ▸ Specify Link…).
    Port(Port),
}

/// The open dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinkDialog {
    pub(crate) target: LinkTarget,
    /// The link chosen so far.
    pub(crate) chosen: Link,
    /// The audio track the buttons select (buttons only).
    pub(crate) audio: Option<u8>,
    /// The subtitle the buttons select (buttons only).
    pub(crate) subtitle: disccraft_project::SubtitleDefault,
}

impl DiscCraftApp {
    /// What Ctrl+L would set for the current selection, the link it starts from, and (for buttons) the audio and
    /// subtitle it selects.
    pub(crate) fn link_dialog_target(
        &self,
    ) -> Option<(
        LinkTarget,
        Link,
        Option<u8>,
        disccraft_project::SubtitleDefault,
    )> {
        let project = &self.session.project;
        if let Some(menu) = self.edit_menu_target()
            && !self.selected_buttons.is_empty()
        {
            let mut buttons = self.selected_buttons.clone();
            buttons.sort_unstable();
            buttons.dedup();
            let primary = self
                .primary_button()
                .and_then(|index| project.menu(menu)?.buttons.get(index));
            let current = primary
                .map(|button| button.link.clone())
                .unwrap_or(Link::NotSet);
            let audio = primary.and_then(|button| button.audio);
            let subtitle = primary.map(|button| button.subtitle).unwrap_or_default();
            return Some((LinkTarget::Buttons(menu, buttons), current, audio, subtitle));
        }
        let (node, current) = match self.selection {
            Selection::Timeline(id) => (
                NodeId::Timeline(id),
                project.timeline(id)?.end_action.clone(),
            ),
            Selection::Slideshow(id) => (
                NodeId::Slideshow(id),
                project.slideshow(id)?.end_action.clone(),
            ),
            Selection::Playlist(id) => (
                NodeId::Playlist(id),
                project.playlist(id)?.end_action.clone(),
            ),
            Selection::ChapterPlaylist(id) => (
                NodeId::ChapterPlaylist(id),
                project.chapter_playlist(id)?.end_action.clone(),
            ),
            Selection::Menu(id) => (NodeId::Menu(id), project.menu(id)?.end_action.clone()),
            Selection::Asset(_) | Selection::None => return None,
        };
        Some((
            LinkTarget::EndAction(node),
            current,
            None,
            disccraft_project::SubtitleDefault::NoChange,
        ))
    }

    /// Opens the dialog for one link field (the ▶ menu's Specify Link…), starting from its current link.
    pub(crate) fn open_link_dialog_for(&mut self, port: Port, current: &Link) {
        let project = &self.session.project;
        let (audio, subtitle) = match port {
            Port::Button { menu, index } => project
                .menu(menu)
                .and_then(|menu| menu.buttons.get(index))
                .map(|button| (button.audio, button.subtitle))
                .unwrap_or_default(),
            _ => (None, disccraft_project::SubtitleDefault::NoChange),
        };
        self.link_dialog = Some(LinkDialog {
            target: LinkTarget::Port(port),
            chosen: current.clone(),
            audio,
            subtitle,
        });
    }

    /// Opens the dialog for the current selection (nothing happens when there is nothing to link).
    pub(crate) fn open_link_dialog(&mut self) {
        if let Some((target, chosen, audio, subtitle)) = self.link_dialog_target() {
            self.link_dialog = Some(LinkDialog {
                target,
                chosen,
                audio,
                subtitle,
            });
        } else {
            self.status = tl!("Select a button or an item with an end action first").to_string();
        }
    }

    /// Draws the dialog while it is open.
    pub(crate) fn link_window(&mut self, ctx: &Context) {
        let Some(mut state) = self.link_dialog.clone() else {
            return;
        };
        let is_button = matches!(
            state.target,
            LinkTarget::Buttons(..) | LinkTarget::Port(Port::Button { .. })
        );
        let chapters = is_button || matches!(state.target, LinkTarget::Port(Port::MenuRemote(_)));
        let targets = link_targets(&self.session.project, chapters);
        let project = self.session.project.clone();
        let mut done = None;
        let response = Modal::new(Id::unique("link_dialog")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading(tl!("Specify Link"));
            let what = match &state.target {
                LinkTarget::Buttons(_, buttons) if buttons.len() > 1 => {
                    tlf!("Link of {n} buttons", n = buttons.len())
                }
                LinkTarget::Buttons(..) | LinkTarget::Port(Port::Button { .. }) => {
                    tl!("Link of the button").to_string()
                }
                LinkTarget::EndAction(_) | LinkTarget::Port(Port::EndAction(_)) => {
                    tl!("End action").to_string()
                }
                LinkTarget::Port(_) => tl!("This link").to_string(),
            };
            ui.label(what);
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .auto_shrink([false, true])
                .show(ui, |ui| link_list(ui, &targets, &mut state.chosen));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(tl!("Link:"));
                ui.strong(link_label(&project, &state.chosen));
            });
            if is_button {
                // Encore のダイアログの「オーディオ」「字幕」: 飛ぶときに選ぶトラック。
                if let Some(audio) = crate::properties::audio_choice_row(
                    ui,
                    tl!("Audio:"),
                    "link_dialog_audio",
                    state.audio,
                ) {
                    state.audio = audio;
                }
                if let Some(subtitle) = crate::properties::subtitle_choice_row(
                    ui,
                    tl!("Subtitle:"),
                    "link_dialog_subtitle",
                    state.subtitle,
                ) {
                    state.subtitle = subtitle;
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(tl!("OK")).clicked() {
                    done = Some(true);
                }
                if ui.button(tl!("Cancel")).clicked() {
                    done = Some(false);
                }
            });
        });
        if response.should_close() {
            done = Some(false);
        }
        match done {
            Some(true) => {
                self.link_dialog = None;
                self.apply_link_dialog(&state);
            }
            Some(false) => self.link_dialog = None,
            None => self.link_dialog = Some(state),
        }
    }

    fn apply_link_dialog(&mut self, state: &LinkDialog) {
        match &state.target {
            LinkTarget::Buttons(menu, buttons) => {
                for index in buttons {
                    let _ = self.run_link_command_pub(
                        "button.setLink",
                        json!({ "menu": menu.0, "button": index }),
                        &state.chosen,
                    );
                    let _ = self.run_command(
                        "button.setStreams",
                        json!({
                            "menu": menu.0,
                            "button": index,
                            "audio": state.audio,
                            "subtitle": crate::properties::subtitle_value(state.subtitle),
                        }),
                    );
                }
            }
            LinkTarget::EndAction(node) => {
                if let Some((command, base)) = command_for_port(&Port::EndAction(*node)) {
                    self.apply_link(command, base, &state.chosen);
                }
            }
            LinkTarget::Port(port) => {
                if let Some((command, base)) = command_for_port(port) {
                    self.apply_link(command, base, &state.chosen);
                }
                if let Port::Button { menu, index } = port {
                    let _ = self.run_command(
                        "button.setStreams",
                        json!({
                            "menu": menu.0,
                            "button": index,
                            "audio": state.audio,
                            "subtitle": crate::properties::subtitle_value(state.subtitle),
                        }),
                    );
                }
            }
        }
    }
}

/// The targets as one list, grouped under headings by kind, with the chosen one highlighted.
fn link_list(ui: &mut Ui, targets: &[(String, Link)], chosen: &mut Link) {
    let groups: [(&str, fn(&Link) -> bool); 6] = [
        (tl!("Timelines"), |link| {
            matches!(link, Link::Timeline { .. })
        }),
        (tl!("Slideshows"), |link| {
            matches!(link, Link::Slideshow { .. })
        }),
        (tl!("Playlists"), |link| {
            matches!(link, Link::Playlist { .. })
        }),
        (tl!("Chapter Playlists"), |link| {
            matches!(link, Link::ChapterPlaylist { .. })
        }),
        (tl!("Menus"), |link| matches!(link, Link::Menu { .. })),
        (tl!("Other"), |link| {
            matches!(link, Link::NotSet | Link::Stop | Link::LastMenu)
        }),
    ];
    for (heading, belongs) in groups {
        let rows: Vec<&(String, Link)> = targets.iter().filter(|(_, link)| belongs(link)).collect();
        if rows.is_empty() {
            continue;
        }
        let open = rows.iter().any(|(_, link)| link == chosen);
        egui::CollapsingHeader::new(heading)
            .default_open(open || rows.len() <= 8)
            .show(ui, |ui| {
                for (name, link) in rows {
                    if ui.selectable_label(link == chosen, name).clicked() {
                        *chosen = link.clone();
                    }
                }
            });
    }
}
