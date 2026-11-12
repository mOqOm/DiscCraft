use egui::{Id, Response, Ui};
use serde_json::{Map, Value};

use crate::DiscCraftApp;
use crate::flowchart::{NodeId, Port, command_for_port, link_to, port_accepts};
use crate::i18n::tl;
use disccraft_project::Link;

/// Payload carried while a pick whip is dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PickWhip(pub(crate) Port);

impl DiscCraftApp {
    /// Draw a pick-whip handle for `port` (tooltip `tl!("Drag onto a menu, timeline, slideshow or playlist in the Project panel to link it")`).
    pub(crate) fn pick_whip(&mut self, ui: &mut Ui, port: Port) {
        let id = Id::unique(("pickwhip", format!("{port:?}")));
        let response = ui
            .dnd_drag_source(id, PickWhip(port), |ui| {
                let size = 12.0;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
                let center = rect.center();
                let radius = size * 0.35;
                let fill = ui.visuals().selection.bg_fill;
                let stroke = egui::Stroke::new(1.0, ui.visuals().text_color());
                let _ = ui.painter().circle_filled(center, radius, fill);
                let _ = ui.painter().circle_stroke(center, radius, stroke);
            })
            .response;
        let _ = response.on_hover_text(tl!(
            "Drag onto a menu, timeline, slideshow or playlist in the Project panel to link it"
        ));
    }

    /// Make `response` (a Project panel row of `node`) a drop target: highlight it while a pick whip hovers over it (a
    /// stroke around `response.rect` using `ui.visuals().selection.stroke`), and on release link the port to `node`
    /// (command_for_port + link_to + apply_link). Returns whether a link was made.
    pub(crate) fn pick_whip_target(&mut self, ui: &Ui, response: &Response, node: NodeId) -> bool {
        if response.dnd_hover_payload::<PickWhip>().is_some() {
            let stroke = ui.visuals().selection.stroke;
            let _ = ui
                .painter()
                .rect_stroke(response.rect, 0.0, stroke, egui::StrokeKind::Inside);
        }

        if let Some(payload) = response.dnd_release_payload::<PickWhip>()
            && let Some((command, base, link)) = link_for_drop(payload.0, node)
        {
            self.apply_link(command, base, &link);
            return true;
        }

        false
    }
}

/// The link a pick whip dropped on `node` creates for `port`, with the command and base params (pure, unit-tested).
pub(crate) fn link_for_drop(
    port: Port,
    node: NodeId,
) -> Option<(&'static str, Map<String, Value>, Link)> {
    if !port_accepts(&port, node) {
        return None;
    }
    let link = link_to(node)?;
    let (command, base) = command_for_port(&port)?;
    Some((command, base, link))
}

#[cfg(test)]
mod tests {
    use super::{Link, NodeId, Port, link_for_drop};
    use disccraft_project::{MenuId, TimelineId};
    use serde_json::Value;

    #[test]
    fn button_drop_on_timeline_creates_button_link() {
        let port = Port::Button {
            menu: MenuId(2),
            index: 3,
        };
        let node = NodeId::Timeline(TimelineId(4));
        let result = link_for_drop(port, node);
        assert!(result.is_some(), "expected a link");
        if let Some((command, base, link)) = result {
            assert_eq!(command, "button.setLink");
            assert_eq!(base.get("menu"), Some(&Value::from(2_u64)));
            assert_eq!(base.get("button"), Some(&Value::from(3_u64)));
            assert_eq!(
                link,
                Link::Timeline {
                    id: TimelineId(4),
                    chapter: 0
                }
            );
        }
    }

    #[test]
    fn end_action_drop_on_menu_creates_menu_end_action() {
        // A timeline's end action dropped on a menu (menus themselves have no end action).
        let port = Port::EndAction(NodeId::Timeline(TimelineId(5)));
        let node = NodeId::Menu(MenuId(6));
        let result = link_for_drop(port, node);
        assert!(result.is_some(), "expected a link");
        if let Some((command, _base, link)) = result {
            assert_eq!(command, "timeline.setEndAction");
            assert_eq!(
                link,
                Link::Menu {
                    id: MenuId(6),
                    button: None
                }
            );
        }
    }

    #[test]
    fn title_button_drop_needs_a_menu() {
        let on_menu = link_for_drop(Port::TitleButton, NodeId::Menu(MenuId(3)));
        assert_eq!(
            on_menu.map(|(command, _, link)| (command, link)),
            Some((
                "nav.setTitleRemote",
                Link::Menu {
                    id: MenuId(3),
                    button: None
                }
            ))
        );
        assert!(link_for_drop(Port::TitleButton, NodeId::Timeline(TimelineId(1))).is_none());
    }

    #[test]
    fn drop_on_first_play_is_none() {
        assert!(link_for_drop(Port::FirstPlay, NodeId::FirstPlay).is_none());
    }
}
