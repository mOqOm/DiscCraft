use egui::{Id, Response, Ui};
use serde_json::{Map, Value};

use crate::DiscCraftApp;
use crate::flowchart::{NodeId, Port, command_for_port, link_to, port_accepts};
use crate::i18n::tl;
use disccraft_project::Link;

/// `response.dnd_release_payload::<T>()` that leaves a dragged item of another type for its own target. egui takes
/// whatever is dragged and keeps it only when it is a `T`, so a drop target that checks for one type first threw away
/// an item of another (M6.434: a pick whip dropped on the flowchart was lost to its check for a dragged node, and a
/// Project panel row dropped on the menu viewer to its check for a Library item).
pub(crate) fn released<T: std::any::Any + Send + Sync>(
    response: &Response,
) -> Option<std::sync::Arc<T>> {
    if egui::DragAndDrop::has_payload_of_type::<T>(&response.ctx) {
        response.dnd_release_payload::<T>()
    } else {
        None
    }
}

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

        if let Some(payload) = released::<PickWhip>(response)
            && let Some((command, base, link)) = link_for_drop(payload.0, node)
        {
            self.apply_link(command, base, &link);
            return true;
        }

        false
    }
}

/// The link a pick whip dropped on a chapter marker or a menu's button creates for `port`: `link` (to a chapter of
/// the timeline `node`, to a button of the menu `node`), with the command and base params (Encore: the pick whip can
/// be dropped on a chapter marker in the Timeline panel and on a button in the Menu viewer).
pub(crate) fn exact_link_for_drop(
    port: Port,
    node: NodeId,
    link: Link,
) -> Option<(&'static str, Map<String, Value>, Link)> {
    if !port_accepts(&port, node) {
        return None;
    }
    let (command, base) = command_for_port(&port)?;
    Some((command, base, link))
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

    /// M6.434: a drop target that first checks for another kind of dragged item still gets a pick whip dropped on it
    /// (egui takes whatever is dragged, so the flowchart's check for a dragged node threw the pick whip away).
    #[test]
    fn a_check_for_another_kind_leaves_the_pick_whip() {
        use super::{PickWhip, released};
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
        let area = egui::Rect::from_min_size(egui::pos2(50.0, 50.0), egui::vec2(100.0, 100.0));
        let at = egui::pos2(100.0, 100.0);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut taken = (None, None);
        {
            let mut frame = |events: Vec<egui::Event>, drop: bool| {
                let input = egui::RawInput {
                    events,
                    screen_rect: Some(screen),
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| {
                    let response = ui.allocate_rect(area, egui::Sense::click_and_drag());
                    if drop {
                        egui::DragAndDrop::set_payload(ui.ctx(), PickWhip(Port::FirstPlay));
                        let other = released::<u8>(&response).is_some();
                        let whip = released::<PickWhip>(&response).map(|whip| whip.0);
                        taken = (Some(other), whip);
                    }
                });
                // No renderer here: the font atlas upload is dropped on purpose.
                output.textures_delta.clear();
            };
            frame(vec![egui::Event::PointerMoved(at)], false);
            frame(vec![button(true)], false);
            frame(vec![button(false)], true);
        }
        assert_eq!(taken, (Some(false), Some(Port::FirstPlay)));
    }

    /// M6.435: a pick whip dropped on a chapter marker links to that chapter, on a menu's button to that button
    /// (Encore); the Title button still takes only a menu.
    #[test]
    fn a_pick_whip_on_a_chapter_or_a_button_links_to_it() {
        use super::exact_link_for_drop;
        let chapter = Link::Timeline {
            id: TimelineId(4),
            chapter: 6,
        };
        let on_marker = exact_link_for_drop(
            Port::EndAction(NodeId::Timeline(TimelineId(9))),
            NodeId::Timeline(TimelineId(4)),
            chapter.clone(),
        );
        assert_eq!(
            on_marker.map(|(command, _, link)| (command, link)),
            Some(("timeline.setEndAction", chapter.clone()))
        );
        let button = Link::Menu {
            id: MenuId(3),
            button: Some(2),
        };
        assert_eq!(
            exact_link_for_drop(Port::TitleButton, NodeId::Menu(MenuId(3)), button.clone())
                .map(|(_, _, link)| link),
            Some(button)
        );
        assert!(
            exact_link_for_drop(Port::TitleButton, NodeId::Timeline(TimelineId(4)), chapter)
                .is_none()
        );
    }

    #[test]
    fn drop_on_first_play_is_none() {
        assert!(link_for_drop(Port::FirstPlay, NodeId::FirstPlay).is_none());
    }
}
