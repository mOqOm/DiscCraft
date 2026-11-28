use egui::{
    Color32, Context, CursorIcon, Id, LayerId, Order, Painter, Pos2, Rect, Response, Sense, Stroke,
    Ui, Vec2,
};
use serde_json::{Map, Value};

use crate::DiscCraftApp;
use crate::flowchart::{NodeId, Port, command_for_port, link_to, port_accepts};
use crate::i18n::tl;
use disccraft_project::{AssetId, AssetKind, Link, MenuId, TimelineId};

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

/// An asset field of the Properties panel that a pick whip fills (M6.520; Encore: the Motion tab's Video field has a
/// pick whip that is dragged onto a video asset).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AssetSlot {
    /// A menu's Motion tab, Video: its moving background.
    MenuVideo(MenuId),
    /// A menu's Motion tab, Audio.
    MenuAudio(MenuId),
    /// A button's Transition tab, Asset: the video played before its link.
    ButtonTransition {
        /// The menu.
        menu: MenuId,
        /// The button's index.
        index: usize,
    },
    /// A menu's Basic tab, Background (M6.526): a picture, or a video (a motion menu, as the Motion tab's Video).
    MenuBackground(MenuId),
    /// A slideshow's audio list (M6.526; Encore: the pick whip beside the Slideshow panel's audio list): an audio
    /// dropped on is added at its end.
    SlideshowAudio(u64),
    /// A chapter's Parental Alternative (M6.526): the video players below its level play in its place.
    ChapterParental {
        /// The timeline.
        timeline: TimelineId,
        /// The chapter's index.
        chapter: usize,
    },
}

impl AssetSlot {
    /// Whether the field takes an asset of `kind` (an EffectCraft composition is a video).
    pub(crate) fn accepts(self, kind: AssetKind) -> bool {
        match self {
            Self::MenuVideo(_) | Self::ButtonTransition { .. } | Self::ChapterParental { .. } => {
                kind == AssetKind::Video
            }
            Self::MenuAudio(_) | Self::SlideshowAudio(_) => kind == AssetKind::Audio,
            Self::MenuBackground(_) => matches!(kind, AssetKind::Image | AssetKind::Video),
        }
    }
}

/// Payload carried while an asset field's pick whip is dragged (M6.520).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AssetWhip(pub(crate) AssetSlot);

/// The pick whip's colour (M6.524): the gold Encore's flowchart edges a drop target with. Encore draws its pick whip's
/// line by inverting what is under it with orange (yellow-green on its grey panels, purple on a light clip); DiscCraft
/// keeps one colour and gives the line a dark edge, so that it reads the same on any ground.
const GOLD: Color32 = crate::flowchart::DROP_EDGE;

/// What a dragged pick whip is over this frame (M6.518, M6.524): the line's colour and the pointer's shape say
/// whether a drop links.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Aim {
    /// Nothing that takes a link: the gold line.
    Nothing,
    /// An item this link cannot go to: the line turns red and the pointer says no (Encore shows nothing there, so
    /// a drop that does nothing looked like one that failed).
    Refused,
    /// An item a drop links to: the gold line, the item marked gold ([`mark_target`]) and the pointer a link.
    Links,
}

impl Aim {
    /// The line's colour and the pointer's shape (so that the colour is not the only sign).
    fn look(self) -> (Color32, CursorIcon) {
        match self {
            Self::Nothing => (GOLD, CursorIcon::Grabbing),
            Self::Refused => (crate::theme::ERROR_TEXT.get(), CursorIcon::NoDrop),
            Self::Links => (GOLD, CursorIcon::Alias),
        }
    }
}

/// Mark `rect` as where a drop links (M6.524): a faint gold fill under a gold edge, the same on every kind of target
/// (Encore fills a Project panel row grey-green, edges a flowchart node gold and marks a menu's button its own way).
pub(crate) fn mark_target(painter: &Painter, rect: Rect) {
    let _ = painter.rect_filled(rect, 2.0, GOLD.gamma_multiply(0.18));
    let _ = painter.rect_stroke(rect, 2.0, Stroke::new(2.0, GOLD), egui::StrokeKind::Inside);
}

/// Frame the field `rect` while its pick whip is dragged (M6.524; Encore: a gold frame round the field, so that it
/// stays clear which field the drop fills).
fn mark_source(painter: &Painter, rect: Rect) {
    let _ = painter.rect_stroke(rect, 2.0, Stroke::new(2.0, GOLD), egui::StrokeKind::Outside);
}

/// Frame `rect`, the field of the link port `port`, while that field's pick whip is dragged.
pub(crate) fn mark_link_source(ui: &Ui, port: Port, rect: Rect) {
    if dragged(ui.ctx()) == Some(port) {
        mark_source(ui.painter(), rect);
    }
}

/// Frame `rect`, the asset field `slot`, while that field's pick whip is dragged.
pub(crate) fn mark_asset_source(ui: &Ui, slot: AssetSlot, rect: Rect) {
    if egui::DragAndDrop::payload::<AssetWhip>(ui.ctx()).is_some_and(|whip| whip.0 == slot) {
        mark_source(ui.painter(), rect);
    }
}

/// Where the tether of the pick whip being dragged starts: the middle of its handle.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tether(Pos2);

fn tether_id() -> Id {
    Id::unique("disccraft_pickwhip_tether")
}

fn aim_id() -> Id {
    Id::unique("disccraft_pickwhip_aim")
}

/// The pick whip being dragged, if any (`None` once Escape took it back or it was dropped).
pub(crate) fn dragged(ctx: &Context) -> Option<Port> {
    egui::DragAndDrop::payload::<PickWhip>(ctx).map(|whip| whip.0)
}

/// Whether any pick whip is being dragged: a link field's or an asset field's.
pub(crate) fn any_dragged(ctx: &Context) -> bool {
    egui::DragAndDrop::has_payload_of_type::<PickWhip>(ctx)
        || egui::DragAndDrop::has_payload_of_type::<AssetWhip>(ctx)
}

/// Over a place that takes links (a menu, a timeline, a chapter marker, a flowchart node): an asset field's pick
/// whip cannot be dropped there, and the tether says so (M6.520).
pub(crate) fn refuse_asset_whip(response: &Response) {
    if response.dnd_hover_payload::<AssetWhip>().is_some() {
        aim(&response.ctx, false);
    }
}

/// Tell the tether what the pick whip is over this frame: an item it links to (`links`) or one it cannot link to. A
/// place that links wins over one that refuses (the drop goes to whichever takes it).
pub(crate) fn aim(ctx: &Context, links: bool) {
    let aim = if links { Aim::Links } else { Aim::Refused };
    ctx.data_mut(|data| {
        let now = data.get_temp::<Aim>(aim_id()).unwrap_or(Aim::Nothing);
        data.insert_temp(aim_id(), now.max(aim));
    });
}

/// A pick-whip handle that carries `payload` (M6.518: dragging it leaves the handle in place and draws a tether from
/// it to the pointer, [`draw_tether`]; Escape takes the drag back).
fn whip_handle<P>(ui: &mut Ui, payload: P) -> Response
where
    P: std::any::Any + Send + Sync + Copy + PartialEq + std::fmt::Debug,
{
    let id = Id::unique(("pickwhip", format!("{payload:?}")));
    let size = 14.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    let response = ui.interact(rect, id, Sense::drag());
    let ctx = ui.ctx().clone();
    if response.drag_started() {
        egui::DragAndDrop::set_payload(&ctx, payload);
    }
    let active = response.dragged()
        && egui::DragAndDrop::payload::<P>(&ctx).is_some_and(|carried| *carried == payload);
    if active {
        // The handle moves with its panel (a scroll while dragging): the tether starts where it is now.
        ctx.data_mut(|data| data.insert_temp(tether_id(), Tether(rect.center())));
    }
    let lit = active || response.hovered();
    let center = rect.center();
    // Gold while it is pointed at or dragged (Encore: the pick whip's mark turns orange).
    let ring = if lit { GOLD } else { ui.visuals().text_color() };
    let painter = ui.painter();
    let _ = painter.circle_filled(center, size * 0.4, ui.visuals().selection.bg_fill);
    let _ = painter.circle_stroke(center, size * 0.4, Stroke::new(1.5, ring));
    let _ = painter.circle_filled(center, size * 0.12, ring);
    if response.hovered() && !active {
        ctx.set_cursor_icon(CursorIcon::Grab);
    }
    response
}

/// Draw the line of the pick whip being dragged, straight from its handle to the pointer as in Encore, above every
/// panel (Encore leaves it out over the flowchart; DiscCraft keeps it), coloured by what it is over, and set the
/// pointer's shape (M6.518, M6.524). Call once a frame after every panel has drawn. Returns what the line showed, if
/// one was drawn.
pub(crate) fn draw_tether(ctx: &Context) -> Option<Aim> {
    let aim = ctx.data_mut(|data| {
        let aim = data.get_temp::<Aim>(aim_id());
        data.remove::<Aim>(aim_id());
        aim.unwrap_or(Aim::Nothing)
    });
    let tether = ctx.data(|data| data.get_temp::<Tether>(tether_id()));
    if !any_dragged(ctx) {
        // Dropped or taken back with Escape: the next drag starts afresh.
        if tether.is_some() {
            ctx.data_mut(|data| data.remove::<Tether>(tether_id()));
        }
        return None;
    }
    let from = tether?.0;
    let to = ctx.pointer_latest_pos()?;
    draw_line(ctx, from, to, aim);
    Some(aim)
}

/// Draw the pick whip's line from `from` to `to` above every panel, coloured by `aim`, and set the pointer's shape
/// (M6.524; also the flowchart's own link drag, M6.525).
pub(crate) fn draw_line(ctx: &Context, from: Pos2, to: Pos2, aim: Aim) {
    let (color, cursor) = aim.look();
    let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, tether_id()));
    // A dark edge under the line keeps it in sight on a light clip or picture as on a grey panel.
    painter.line_segment([from, to], Stroke::new(4.5, Color32::from_black_alpha(150)));
    painter.line_segment([from, to], Stroke::new(2.5, color));
    painter.circle_filled(to, 3.0, color);
    ctx.set_cursor_icon(cursor);
}

/// How far a list scrolls in a sixtieth of a second when a dragged pick whip is held `inset` points from its edge,
/// within `EDGE` (Encore: a list scrolls while an item is dragged near its edge). Faster nearer the edge, nothing
/// outside it.
fn edge_speed(inset: f32) -> f32 {
    const EDGE: f32 = 24.0;
    const FASTEST: f32 = 14.0;
    if !(0.0..EDGE).contains(&inset) {
        return 0.0;
    }
    FASTEST * (1.0 - inset / EDGE)
}

/// The scroll a dragged pick whip at `point` asks of a list showing `view` (content moves down to show what is above).
fn edge_scroll(view: Rect, point: Pos2) -> Vec2 {
    if !view.contains(point) {
        return Vec2::ZERO;
    }
    Vec2::new(
        edge_speed(point.x - view.min.x) - edge_speed(view.max.x - point.x),
        edge_speed(point.y - view.min.y) - edge_speed(view.max.y - point.y),
    )
}

/// The scroll a pick whip held near the edge of `view` asks for this frame (M6.518, M6.525): the content moves by it
/// (down to show what is above), the same distance a second at any frame rate. Zero unless a pick whip, or a link
/// the caller is `dragging` (the flowchart's own), is being dragged.
pub(crate) fn edge_delta(ctx: &Context, view: Rect, dragging: bool) -> Vec2 {
    if !(dragging || any_dragged(ctx)) {
        return Vec2::ZERO;
    }
    let Some(point) = ctx.pointer_latest_pos() else {
        return Vec2::ZERO;
    };
    // The speeds are per sixtieth of a second, the same at any frame rate.
    let frames = ctx.input(|input| input.stable_dt).clamp(0.0, 0.1) * 60.0;
    edge_scroll(view, point) * frames
}

/// Inside a `ScrollArea`'s contents: scroll it while a pick whip is dragged near its edge (M6.518), so an item out of
/// sight can be reached.
pub(crate) fn scroll_near_edges(ui: &Ui) {
    scroll_near_edges_for(ui, false);
}

/// [`scroll_near_edges`], also while a link the caller is `dragging` (the flowchart's own link drag, M6.525).
pub(crate) fn scroll_near_edges_for(ui: &Ui, dragging: bool) {
    let delta = edge_delta(ui.ctx(), ui.clip_rect(), dragging);
    if delta != Vec2::ZERO {
        ui.scroll_with_delta(delta);
        ui.ctx().request_repaint();
    }
}

impl DiscCraftApp {
    /// Draw a pick-whip handle for `port`. Its tooltip names every place it can be dropped (M6.525: it named only the
    /// Project panel).
    pub(crate) fn pick_whip(&mut self, ui: &mut Ui, port: Port) {
        let response = whip_handle(ui, PickWhip(port));
        let _ = response.on_hover_text(tl!(
            "Drag onto an item in the Project panel, a box in the Flowchart, a button in the Menu Viewer or a chapter marker to link it"
        ));
    }

    /// Draw the pick-whip handle of an asset field (M6.520): dragged onto an asset of the kind the field takes in the
    /// Project panel, it puts that asset in the field.
    pub(crate) fn asset_pick_whip(&mut self, ui: &mut Ui, slot: AssetSlot) {
        let response = whip_handle(ui, AssetWhip(slot));
        let _ = response.on_hover_text(tl!("Drag onto an asset in the Project panel to use it"));
    }

    /// Make `response` (a Project panel row of the asset `asset`) a target of asset fields' pick whips (M6.520): the
    /// tether says whether the field takes this kind of asset, the row is outlined when it does, and a release puts
    /// the asset in the field. A link field's pick whip cannot link to an asset: red. Returns whether a field took it.
    pub(crate) fn asset_whip_target(
        &mut self,
        ui: &Ui,
        response: &Response,
        asset: AssetId,
    ) -> bool {
        let kind = self
            .session
            .project
            .assets
            .get(&asset)
            .map(|found| found.kind);
        if response.dnd_hover_payload::<PickWhip>().is_some() {
            aim(ui.ctx(), false);
        }
        if let Some(payload) = response.dnd_hover_payload::<AssetWhip>() {
            let takes = kind.is_some_and(|kind| payload.0.accepts(kind));
            aim(ui.ctx(), takes);
            if takes {
                mark_target(ui.painter(), response.rect);
            }
        }
        if let Some(payload) = released::<AssetWhip>(response)
            && kind.is_some_and(|kind| payload.0.accepts(kind))
        {
            self.fill_asset_slot(payload.0, asset);
            return true;
        }
        false
    }

    /// Put `asset` in the asset field `slot`, as choosing it in the field's list does (nothing when it is there; a
    /// slideshow's audio list takes it at its end).
    fn fill_asset_slot(&mut self, slot: AssetSlot, asset: AssetId) {
        let project = &self.session.project;
        let kind = project.assets.get(&asset).map(|found| found.kind);
        match slot {
            AssetSlot::MenuBackground(menu) if kind == Some(AssetKind::Image) => {
                let Some(found) = project.menu(menu) else {
                    return;
                };
                if matches!(found.background, disccraft_project::MenuBackground::Image { asset: now } if now == asset)
                {
                    return;
                }
                self.set_background_image(menu, asset);
            }
            AssetSlot::MenuBackground(menu) => {
                self.fill_asset_slot(AssetSlot::MenuVideo(menu), asset)
            }
            AssetSlot::SlideshowAudio(slideshow) => {
                let _ = self.run_command(
                    "slideshow.addAudioClip",
                    serde_json::json!({ "slideshow": slideshow, "asset": asset.0 }),
                );
            }
            AssetSlot::ChapterParental { timeline, chapter } => {
                let current = project.timeline(timeline).and_then(|found| {
                    found
                        .parental_chapters
                        .iter()
                        .find(|parental| parental.chapter == chapter)
                        .copied()
                });
                if current.is_some_and(|parental| parental.alternative == asset) {
                    return;
                }
                // A first alternative is for levels below 8, as choosing it in the field's list makes it.
                let level = current.map_or(8, |parental| parental.level);
                let _ = self.run_command(
                    "timeline.setParentalChapter",
                    serde_json::json!({
                        "timeline": timeline.0, "chapter": chapter, "level": level, "asset": asset.0
                    }),
                );
            }
            AssetSlot::MenuVideo(menu) => {
                let Some(found) = project.menu(menu) else {
                    return;
                };
                let seconds = match found.background {
                    disccraft_project::MenuBackground::Video {
                        asset: now,
                        seconds,
                        ..
                    } => {
                        if now == asset {
                            return;
                        }
                        seconds
                    }
                    _ => crate::properties::DEFAULT_LOOP_SECONDS,
                };
                self.set_background_video(menu, asset, seconds);
            }
            AssetSlot::MenuAudio(menu) => {
                if project
                    .menu(menu)
                    .is_none_or(|found| found.audio == Some(asset))
                {
                    return;
                }
                let _ = self.run_command(
                    "menu.setAudio",
                    serde_json::json!({ "menu": menu.0, "asset": asset.0 }),
                );
            }
            AssetSlot::ButtonTransition { menu, index } => {
                let Some(button) = project
                    .menu(menu)
                    .and_then(|found| found.buttons.get(index))
                else {
                    return;
                };
                if button.transition == Some(asset) {
                    return;
                }
                let _ = self.run_command(
                    "button.setTransition",
                    serde_json::json!({ "menu": menu.0, "button": index, "asset": asset.0 }),
                );
            }
        }
    }

    /// Make `response` (a Project panel row of `node`) a drop target: while a pick whip hovers over it, tell the
    /// tether whether it links here and mark it when it does ([`mark_target`]), and on release
    /// link the port to `node` (command_for_port + link_to + apply_link). Returns whether a link was made.
    pub(crate) fn pick_whip_target(&mut self, ui: &Ui, response: &Response, node: NodeId) -> bool {
        refuse_asset_whip(response);
        if let Some(payload) = response.dnd_hover_payload::<PickWhip>() {
            let links = link_for_drop(payload.0, node).is_some();
            aim(ui.ctx(), links);
            if links {
                mark_target(ui.painter(), response.rect);
            }
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

    /// M6.524: the line is the gold of a flowchart drop target over nothing and over a place it links to (Encore keeps
    /// one colour too), red where it cannot link (Encore shows nothing there); the pointer's shape says the same.
    #[test]
    fn the_line_is_gold_and_red_where_it_cannot_link() {
        use super::{Aim, GOLD};
        assert_eq!(GOLD, crate::flowchart::DROP_EDGE);
        assert_eq!(Aim::Nothing.look(), (GOLD, egui::CursorIcon::Grabbing));
        assert_eq!(Aim::Links.look(), (GOLD, egui::CursorIcon::Alias));
        let (red, cursor) = Aim::Refused.look();
        assert_ne!(red, GOLD);
        assert_eq!(cursor, egui::CursorIcon::NoDrop);
    }

    /// M6.518: a list scrolls while a pick whip is held near its edge, faster nearer the edge (content moves down to
    /// show what is above), and not at all in the middle or outside.
    #[test]
    fn a_list_scrolls_while_a_pick_whip_is_held_near_its_edge() {
        use super::edge_scroll;
        let view = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(200.0, 400.0));
        assert_eq!(
            edge_scroll(view, egui::pos2(100.0, 200.0)),
            egui::Vec2::ZERO
        );
        assert_eq!(
            edge_scroll(view, egui::pos2(100.0, 500.0)),
            egui::Vec2::ZERO
        );
        let top = edge_scroll(view, egui::pos2(100.0, 2.0));
        let nearer = edge_scroll(view, egui::pos2(100.0, 0.0));
        assert!(top.y > 0.0 && nearer.y > top.y && top.x == 0.0);
        let bottom = edge_scroll(view, egui::pos2(100.0, 390.0));
        assert!(bottom.y < 0.0);
        let left = edge_scroll(view, egui::pos2(5.0, 200.0));
        assert!(left.x > 0.0 && left.y == 0.0);
        assert_eq!(
            edge_scroll(view, egui::pos2(100.0, 376.0)),
            egui::Vec2::ZERO
        );
    }

    /// M6.518: dragging the handle carries the pick whip and draws a tether coloured by what it is over (a place
    /// that links wins); Escape takes it back for the rest of the drag, so a release then links nothing (egui's drag
    /// source put it back every frame while the button was held).
    #[test]
    fn escape_takes_a_dragged_pick_whip_back() {
        use super::{Aim, PickWhip, aim, dragged, draw_tether, whip_handle};
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
        let center = std::cell::Cell::new(egui::Pos2::ZERO);
        let frame = |events: Vec<egui::Event>, report: Option<bool>| {
            let input = egui::RawInput {
                events,
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut seen = (None, None);
            let mut output = ctx.run_ui(input, |ui| {
                center.set(whip_handle(ui, PickWhip(Port::TitleButton)).rect.center());
                if let Some(links) = report {
                    aim(ui.ctx(), true);
                    aim(ui.ctx(), links);
                }
                seen = (dragged(ui.ctx()), draw_tether(ui.ctx()));
            });
            // No renderer here: the font atlas upload is dropped on purpose.
            output.textures_delta.clear();
            seen
        };
        let button = |at, pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(frame(vec![], None), (None, None));
        let at = center.get();
        let _ = frame(vec![egui::Event::PointerMoved(at)], None);
        // A handle only drags, so egui starts the drag at the press.
        assert_eq!(
            frame(vec![button(at, true)], None),
            (Some(Port::TitleButton), Some(Aim::Nothing))
        );
        let away = at + egui::vec2(60.0, 40.0);
        assert_eq!(
            frame(vec![egui::Event::PointerMoved(away)], None),
            (Some(Port::TitleButton), Some(Aim::Nothing))
        );
        assert_eq!(
            frame(
                vec![egui::Event::PointerMoved(away + egui::vec2(5.0, 0.0))],
                Some(false)
            ),
            (Some(Port::TitleButton), Some(Aim::Links))
        );
        let escape = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(frame(vec![escape], None), (None, None));
        let later = away + egui::vec2(20.0, 0.0);
        assert_eq!(
            frame(vec![egui::Event::PointerMoved(later)], None),
            (None, None)
        );
        assert_eq!(frame(vec![button(later, false)], None), (None, None));
    }

    /// M6.520, M6.526: the Motion tab's Video field, a button's transition and a chapter's parental alternative take
    /// a video (an EffectCraft composition is one), the Motion tab's Audio field and a slideshow's audio list a sound,
    /// a menu's background a picture or a video; nothing else.
    #[test]
    fn asset_fields_take_their_kind_of_asset() {
        use super::AssetSlot;
        use disccraft_project::AssetKind::{Audio, Image, Other, Subtitle, Video};
        let menu = MenuId(3);
        let takes = |slot: AssetSlot| {
            [Video, Audio, Image, Subtitle, Other]
                .into_iter()
                .filter(|kind| slot.accepts(*kind))
                .collect::<Vec<_>>()
        };
        assert_eq!(takes(AssetSlot::MenuVideo(menu)), vec![Video]);
        assert_eq!(takes(AssetSlot::MenuAudio(menu)), vec![Audio]);
        assert_eq!(
            takes(AssetSlot::ButtonTransition { menu, index: 1 }),
            vec![Video]
        );
        assert_eq!(takes(AssetSlot::MenuBackground(menu)), vec![Video, Image]);
        assert_eq!(takes(AssetSlot::SlideshowAudio(4)), vec![Audio]);
        assert_eq!(
            takes(AssetSlot::ChapterParental {
                timeline: TimelineId(1),
                chapter: 0
            }),
            vec![Video]
        );
    }

    /// M6.520: an asset field's pick whip draws the same tether and is taken back by Escape too; a place that takes
    /// links refuses it (red).
    #[test]
    fn an_asset_fields_pick_whip_is_refused_by_link_targets() {
        use super::{Aim, AssetSlot, AssetWhip, draw_tether, refuse_asset_whip, whip_handle};
        let ctx = egui::Context::default();
        let slot = AssetWhip(AssetSlot::MenuVideo(MenuId(2)));
        let target = egui::Rect::from_min_size(egui::pos2(200.0, 100.0), egui::vec2(100.0, 40.0));
        let center = std::cell::Cell::new(egui::Pos2::ZERO);
        let frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut seen = None;
            let mut output = ctx.run_ui(input, |ui| {
                center.set(whip_handle(ui, slot).rect.center());
                let response = ui.allocate_rect(target, egui::Sense::hover());
                refuse_asset_whip(&response);
                seen = draw_tether(ui.ctx());
            });
            // No renderer here: the font atlas upload is dropped on purpose.
            output.textures_delta.clear();
            seen
        };
        let _ = frame(vec![]);
        let at = center.get();
        let press = egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        };
        let _ = frame(vec![egui::Event::PointerMoved(at)]);
        assert_eq!(frame(vec![press]), Some(Aim::Nothing));
        assert_eq!(
            frame(vec![egui::Event::PointerMoved(target.center())]),
            Some(Aim::Refused)
        );
    }

    /// M6.520, M6.526: an asset dropped on an asset field goes in it as choosing it in the field's list does (the
    /// Motion tab's video and audio, a button's transition, a menu's background, a slideshow's audio list); the same
    /// asset again changes nothing (no step to undo).
    #[test]
    fn an_asset_dropped_on_a_field_goes_in_it() -> Result<(), Box<dyn std::error::Error>> {
        use super::AssetSlot;
        use crate::app::DiscCraftApp;
        use disccraft_project::MenuBackground;
        use serde_json::json;
        use std::path::Path;
        let mut app = DiscCraftApp::new(&egui::Context::default());
        let created = app
            .session
            .execute("menu.create", &json!({ "name": "M" }))?;
        let id = MenuId(
            created
                .get("menu")
                .and_then(Value::as_u64)
                .ok_or("no menu id")?,
        );
        let _ = app
            .session
            .execute("menu.addButton", &json!({ "menu": id.0, "label": "Play" }))?;
        // Files that are not there: the assets are offline, which a drop does not mind.
        let video = app.session.project.add_asset(Path::new("loop.mp4"))?;
        let sound = app.session.project.add_asset(Path::new("theme.wav"))?;
        let picture = app.session.project.add_asset(Path::new("sky.png"))?;
        app.fill_asset_slot(AssetSlot::MenuVideo(id), video);
        app.fill_asset_slot(AssetSlot::MenuAudio(id), sound);
        app.fill_asset_slot(AssetSlot::ButtonTransition { menu: id, index: 0 }, video);
        let menu = app.session.project.menu(id).ok_or("no menu")?;
        assert!(
            matches!(menu.background, MenuBackground::Video { asset, .. } if asset == video),
            "{:?}",
            menu.background
        );
        assert_eq!(menu.audio, Some(sound));
        assert_eq!(
            menu.buttons.first().and_then(|button| button.transition),
            Some(video)
        );
        let steps = app.session.undo_labels().len();
        app.fill_asset_slot(AssetSlot::MenuVideo(id), video);
        app.fill_asset_slot(AssetSlot::MenuAudio(id), sound);
        assert_eq!(app.session.undo_labels().len(), steps);

        // M6.526: a picture dropped on the background makes it the menu's picture, a video its moving background; a
        // sound dropped on a slideshow's audio list is added at its end, each time.
        app.fill_asset_slot(AssetSlot::MenuBackground(id), picture);
        let menu = app.session.project.menu(id).ok_or("no menu")?;
        assert_eq!(menu.background, MenuBackground::Image { asset: picture });
        app.fill_asset_slot(AssetSlot::MenuBackground(id), video);
        let menu = app.session.project.menu(id).ok_or("no menu")?;
        assert!(matches!(menu.background, MenuBackground::Video { asset, .. } if asset == video));
        let created = app.session.execute("slideshow.create", &json!({}))?;
        let slideshow = created
            .get("slideshow")
            .and_then(Value::as_u64)
            .ok_or("no slideshow id")?;
        app.fill_asset_slot(AssetSlot::SlideshowAudio(slideshow), sound);
        app.fill_asset_slot(AssetSlot::SlideshowAudio(slideshow), sound);
        let clips = app
            .session
            .project
            .slideshows
            .get(&slideshow)
            .map(|found| found.audio_clips.clone());
        assert_eq!(clips, Some(vec![sound, sound]));
        Ok(())
    }

    /// M6.525: a pick whip, or a flowchart link being dragged, held near an edge asks for a scroll; nothing dragged,
    /// none.
    #[test]
    fn only_a_drag_near_an_edge_scrolls() {
        use super::edge_delta;
        let ctx = egui::Context::default();
        let view = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(300.0, 200.0));
        let mut seen = (egui::Vec2::ZERO, egui::Vec2::ZERO);
        let input = egui::RawInput {
            events: vec![egui::Event::PointerMoved(egui::pos2(295.0, 100.0))],
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            seen = (
                edge_delta(ui.ctx(), view, false),
                edge_delta(ui.ctx(), view, true),
            );
        });
        // No renderer here: the font atlas upload is dropped on purpose.
        output.textures_delta.clear();
        assert_eq!(seen.0, egui::Vec2::ZERO);
        assert!(seen.1.x < 0.0 && seen.1.y == 0.0, "{:?}", seen.1);
    }

    /// M6.530: a link set by a drop (or a link field) names its step after the place it goes to, so Edit ▸ Undo says
    /// where; the name goes with the step to Redo.
    #[test]
    fn a_link_step_is_named_after_its_place() -> Result<(), Box<dyn std::error::Error>> {
        use crate::app::DiscCraftApp;
        use serde_json::json;
        use std::path::Path;
        let mut app = DiscCraftApp::new(&egui::Context::default());
        let asset = app.session.project.add_asset(Path::new("making.mp4"))?;
        let timeline = app
            .session
            .project
            .add_timeline(asset, Some("Making".to_string()))?;
        let created = app
            .session
            .execute("menu.create", &json!({ "name": "Main" }))?;
        let menu = MenuId(
            created
                .get("menu")
                .and_then(Value::as_u64)
                .ok_or("no menu id")?,
        );
        let _ = app.session.execute(
            "menu.addButton",
            &json!({ "menu": menu.0, "label": "Play" }),
        )?;
        let link = Link::Timeline {
            id: timeline,
            chapter: 0,
        };
        let (command, base, link) =
            super::link_for_drop(Port::Button { menu, index: 0 }, NodeId::Timeline(timeline))
                .map(|(command, base, _)| (command, base, link))
                .ok_or("no link")?;
        app.apply_link(command, base, &link);
        let place = crate::menu_editor::link_label(&app.session.project, &link);
        assert!(place.contains("Making"), "{place}");
        assert_eq!(app.session.undo_detail(), Some(place.as_str()));
        app.session.undo()?;
        assert_eq!(app.session.redo_detail(), Some(place.as_str()));
        Ok(())
    }

    #[test]
    fn drop_on_first_play_is_none() {
        assert!(link_for_drop(Port::FirstPlay, NodeId::FirstPlay).is_none());
    }
}
