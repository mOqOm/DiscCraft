//! Flowchart view for disc navigation.

use crate::app::{DiscCraftApp, Selection};
use crate::gui_params::with_target;
use crate::i18n::{tl, tlf};
use crate::menu_editor::link_label;
use disccraft_project::{AssetId, Link, Menu, MenuBackground, MenuId, Project, TimelineId};
use egui::{
    Align2, FontId, Id, Painter, Pos2, Rect, Response, ScrollArea, Sense, Stroke, StrokeKind,
    TextureHandle, Ui, Vec2, Visuals, vec2,
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Width of a flowchart node.
const NODE_WIDTH: f32 = 180.0;
/// Height of a node header.
const HEADER: f32 = 24.0;
/// Height of one port row.
const ROW: f32 = 18.0;
/// Vertical gap between nodes in a column.
const GAP: f32 = 16.0;
/// Horizontal gap between columns.
const COLUMN_GAP: f32 = 80.0;
/// Padding around the whole graph.
const PADDING: f32 = 24.0;
/// Corner radius for node rectangles.
const RADIUS: f32 = 6.0;
/// Left padding for port text.
const PAD: f32 = 8.0;
/// Height of the thumbnail band of nodes with media.
const THUMB: f32 = 64.0;
/// Radius of a port circle.
const PORT_RADIUS: f32 = 4.0;
/// Hit radius for a port circle.
const PORT_HIT: f32 = 8.0;

/// A node of the navigation graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NodeId {
    /// First play entry point.
    FirstPlay,
    /// A menu node.
    Menu(MenuId),
    /// A timeline node.
    Timeline(TimelineId),
    /// A slideshow node.
    Slideshow(u64),
    /// A playlist node.
    Playlist(u64),
    /// A chapter playlist.
    ChapterPlaylist(u64),
}

/// Where a link starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Port {
    /// First play port.
    FirstPlay,
    /// A menu button port.
    Button {
        /// Menu containing the button.
        menu: MenuId,
        /// Button index inside the menu.
        index: usize,
    },
    /// An end-action port on a node.
    EndAction(NodeId),
    /// The disc's Title button (the remote's Title / Top Menu key). It can only open a menu.
    TitleButton,
    /// A timeline's remote menu (the remote's Menu key while the timeline plays).
    MenuRemote(TimelineId),
    /// A timeline's override: where to go after the title its end action leads to.
    Override(TimelineId),
    /// A menu's override: where to go after the title its end action plays.
    MenuOverride(MenuId),
    /// The disc's override: where to go after the title the first play plays.
    DiscOverride,
    /// A button's override: where to go after the title the button plays.
    ButtonOverride {
        /// Menu containing the button.
        menu: MenuId,
        /// Button index inside the menu.
        index: usize,
    },
    /// A chapter playlist's override: where to go after the title its end action leads to.
    ChapterPlaylistOverride(u64),
    /// A playlist's item list: a drop appends the title (timeline, slideshow or chapter playlist).
    PlaylistItems(u64),
}

/// Whether a link from `port` may point at `node` (the Title button only opens menus).
pub(crate) fn port_accepts(port: &Port, node: NodeId) -> bool {
    match port {
        Port::TitleButton => matches!(node, NodeId::Menu(_)),
        Port::PlaylistItems(_) => matches!(
            node,
            NodeId::Timeline(_) | NodeId::Slideshow(_) | NodeId::ChapterPlaylist(_)
        ),
        Port::FirstPlay
        | Port::Button { .. }
        | Port::EndAction(_)
        | Port::MenuRemote(_)
        | Port::Override(_)
        | Port::MenuOverride(_)
        | Port::DiscOverride
        | Port::ChapterPlaylistOverride(_)
        | Port::ButtonOverride { .. } => node != NodeId::FirstPlay,
    }
}

/// A flowchart node with its visible ports.
#[derive(Clone, Debug)]
pub(crate) struct Node {
    /// Graph node identity.
    pub(crate) id: NodeId,
    /// Header title.
    pub(crate) title: String,
    /// Ports with display text.
    pub(crate) ports: Vec<(Port, String)>,
    /// Asset shown as a thumbnail under the header (a timeline's video, a slideshow's first slide, a menu's
    /// background image or video).
    pub(crate) media: Option<AssetId>,
}

/// A navigation edge between a port and a target node.
#[derive(Clone, Debug)]
pub(crate) struct Edge {
    /// Source port.
    pub(crate) from: Port,
    /// Target node.
    pub(crate) to: NodeId,
    /// Optional edge label.
    pub(crate) label: String,
}

/// A link drag in progress.
#[derive(Clone, Copy)]
struct FlowDrag {
    /// Port being dragged from.
    port: Port,
}

/// Build nodes and edges from the project.
pub(crate) fn build_graph(project: &Project) -> (Vec<Node>, Vec<Edge>) {
    let mut nodes = vec![first_play_node(project)];

    for menu in project.menus.values() {
        nodes.push(menu_node(project, menu));
    }

    for timeline in project.timelines.values() {
        let title = object_title(&timeline.name, tlf!("Timeline {id}", id = timeline.id.0));
        let mut node = end_action_node(
            NodeId::Timeline(timeline.id),
            title,
            &timeline.end_action,
            project,
        );
        node.media = Some(timeline.video);
        nodes.push(node);
    }

    for slideshow in project.slideshows.values() {
        let title = object_title(&slideshow.name, tlf!("Slideshow {id}", id = slideshow.id));
        let mut node = end_action_node(
            NodeId::Slideshow(slideshow.id),
            title,
            &slideshow.end_action,
            project,
        );
        node.media = slideshow.slides.first().map(|slide| slide.asset);
        nodes.push(node);
    }

    for playlist in project.playlists.values() {
        let title = object_title(&playlist.name, tlf!("Playlist {id}", id = playlist.id));
        nodes.push(end_action_node(
            NodeId::Playlist(playlist.id),
            title,
            &playlist.end_action,
            project,
        ));
    }

    for playlist in project.chapter_playlists.values() {
        let title = object_title(
            &playlist.name,
            tlf!("Chapter playlist {id}", id = playlist.id),
        );
        let mut node = end_action_node(
            NodeId::ChapterPlaylist(playlist.id),
            title,
            &playlist.end_action,
            project,
        );
        node.media = project
            .timelines
            .get(&playlist.timeline)
            .map(|timeline| timeline.video);
        nodes.push(node);
    }

    let edges = collect_edges(project, &nodes);
    (nodes, edges)
}

/// Lay nodes out in columns.
pub(crate) fn layout(nodes: &[Node], origin: Pos2) -> Vec<(NodeId, Rect)> {
    let mut sorted: Vec<&Node> = nodes.iter().collect();
    sorted.sort_by_key(|node| (column_of(node.id), node.id));

    let mut ys = [origin.y; 4];
    let mut rects = Vec::with_capacity(nodes.len());

    for node in sorted {
        let column = column_of(node.id);
        let height = node_height(node);
        let y = match ys.get(column) {
            Some(y) => *y,
            None => origin.y,
        };
        let x = column_x(origin, column);
        let rect = Rect::from_min_size(Pos2::new(x, y), vec2(NODE_WIDTH, height));
        rects.push((node.id, rect));

        if let Some(slot) = ys.get_mut(column) {
            *slot = y + height + GAP;
        }
    }

    rects
}

/// Whether `node` is linked from somewhere (the disc node always counts as linked).
fn is_linked(node: NodeId, edges: &[Edge]) -> bool {
    node == NodeId::FirstPlay || edges.iter().any(|edge| edge.to == node)
}

/// Layout like Encore's flowchart: the linked graph on top, and the items nothing links to in a row below it.
/// Returns the node rectangles and, when there are unlinked items, the y of the divider above their row.
pub(crate) fn layout_split(
    nodes: &[Node],
    edges: &[Edge],
    origin: Pos2,
) -> (Vec<(NodeId, Rect)>, Option<f32>) {
    let (linked, mut unlinked): (Vec<&Node>, Vec<&Node>) =
        nodes.iter().partition(|node| is_linked(node.id, edges));
    let linked: Vec<Node> = linked.into_iter().cloned().collect();
    let mut rects = layout(&linked, origin);
    if unlinked.is_empty() {
        return (rects, None);
    }

    let bottom = rects
        .iter()
        .map(|(_, rect)| rect.max.y)
        .fold(origin.y, f32::max);
    // Leave room for the lanes of back links drawn below the linked nodes.
    let divider = bottom + BACK_LANE_GAP * 9.0 + PAD;
    let top = divider + UNLINKED_HEADING;

    unlinked.sort_by_key(|node| (column_of(node.id), node.id));
    let mut x = column_x(origin, 0);
    for node in unlinked {
        let rect = Rect::from_min_size(Pos2::new(x, top), vec2(NODE_WIDTH, node_height(node)));
        rects.push((node.id, rect));
        x += NODE_WIDTH + GAP;
    }
    (rects, Some(divider))
}

/// Height of the "Unlinked items" heading above their row.
const UNLINKED_HEADING: f32 = 28.0;

/// The link a drop on `node` creates (`None` for First Play).
pub(crate) fn link_to(node: NodeId) -> Option<Link> {
    match node {
        NodeId::FirstPlay => None,
        NodeId::Menu(id) => Some(Link::Menu { id, button: None }),
        NodeId::Timeline(id) => Some(Link::Timeline { id, chapter: 0 }),
        NodeId::Slideshow(id) => Some(Link::Slideshow { id }),
        NodeId::Playlist(id) => Some(Link::Playlist { id }),
        NodeId::ChapterPlaylist(id) => Some(Link::ChapterPlaylist { id }),
    }
}

/// Build the disc node with its First Play and Title button ports.
fn first_play_node(project: &Project) -> Node {
    let first_play = &project.settings.first_play;
    let first_label = tl!("First Play");
    let first_target = resolve_target(project, first_play);
    let title = &project.settings.title_remote;
    let title_label = tl!("Title Button");
    let title_target = resolve_target(project, title);

    Node {
        id: NodeId::FirstPlay,
        title: tl!("Disc").to_owned(),
        ports: vec![
            (
                Port::FirstPlay,
                port_text(project, first_label, first_play, first_target),
            ),
            (
                Port::TitleButton,
                port_text(project, title_label, title, title_target),
            ),
        ],
        media: None,
    }
}

/// Build a menu node with one port per button.
fn menu_node(project: &Project, menu: &Menu) -> Node {
    let ports = menu
        .buttons
        .iter()
        .enumerate()
        .map(|(index, button)| {
            let label = if button.label.is_empty() {
                tlf!("Button {n}", n = index.saturating_add(1))
            } else {
                button.label.clone()
            };
            let target = resolve_target(project, &button.link);
            (
                Port::Button {
                    menu: menu.id,
                    index,
                },
                port_text(project, &label, &button.link, target),
            )
        })
        .collect();

    Node {
        id: NodeId::Menu(menu.id),
        title: object_title(
            &crate::menu_editor::menu_title(project, menu.id),
            tlf!("Menu {id}", id = menu.id.0),
        ),
        ports,
        media: menu_media(menu),
    }
}

/// The asset a menu node shows as its thumbnail (the background image or motion video).
fn menu_media(menu: &Menu) -> Option<AssetId> {
    match &menu.background {
        MenuBackground::Image { asset } | MenuBackground::Video { asset, .. } => Some(*asset),
        MenuBackground::Color { .. } => None,
    }
}

/// Build a node whose only port is an end action.
fn end_action_node(id: NodeId, title: String, link: &Link, project: &Project) -> Node {
    let label = tl!("End Action");
    let target = resolve_target(project, link);

    Node {
        id,
        title,
        ports: vec![(Port::EndAction(id), port_text(project, label, link, target))],
        media: None,
    }
}

/// Collect edges for all ports that resolve to existing nodes.
fn collect_edges(project: &Project, nodes: &[Node]) -> Vec<Edge> {
    let mut edges = Vec::new();

    for node in nodes {
        for (port, _) in &node.ports {
            let Some(link) = source_link(project, port) else {
                continue;
            };
            let Some(to) = resolve_target(project, link) else {
                continue;
            };

            edges.push(Edge {
                from: *port,
                to,
                label: edge_label(link),
            });
        }
    }

    edges
}

/// The divider and heading above the row of unlinked items.
fn draw_unlinked_heading(painter: &Painter, ui: &Ui, area: Rect, y: f32) {
    let visuals = ui.visuals();
    painter.line_segment(
        [Pos2::new(area.min.x, y), Pos2::new(area.max.x, y)],
        visuals.widgets.noninteractive.bg_stroke,
    );
    painter.text(
        Pos2::new(area.min.x + PAD, y + 4.0),
        egui::Align2::LEFT_TOP,
        tl!("Unlinked items"),
        egui::FontId::proportional(12.0),
        visuals.weak_text_color(),
    );
}

/// Return the link stored at a port, if it still exists.
fn source_link<'a>(project: &'a Project, port: &Port) -> Option<&'a Link> {
    match port {
        Port::FirstPlay => Some(&project.settings.first_play),
        Port::TitleButton => Some(&project.settings.title_remote),
        Port::MenuRemote(id) => project
            .timelines
            .get(id)
            .map(|timeline| &timeline.menu_remote),
        Port::Override(id) => project
            .timelines
            .get(id)
            .and_then(|timeline| timeline.override_link.as_ref()),
        Port::MenuOverride(id) => project
            .menus
            .get(id)
            .and_then(|menu| menu.override_link.as_ref()),
        Port::DiscOverride => project.settings.first_play_override.as_ref(),
        Port::ChapterPlaylistOverride(id) => project
            .chapter_playlists
            .get(id)
            .and_then(|playlist| playlist.override_link.as_ref()),
        // An item list holds many links; it is never drawn as a single edge.
        Port::PlaylistItems(_) => None,
        Port::ButtonOverride { menu, index } => project
            .menus
            .get(menu)?
            .buttons
            .get(*index)
            .and_then(|button| button.override_link.as_ref()),
        Port::Button { menu, index } => project
            .menus
            .get(menu)?
            .buttons
            .get(*index)
            .map(|button| &button.link),
        Port::EndAction(source) => match source {
            NodeId::Timeline(id) => project
                .timelines
                .get(id)
                .map(|timeline| &timeline.end_action),
            NodeId::Slideshow(id) => project
                .slideshows
                .get(id)
                .map(|slideshow| &slideshow.end_action),
            NodeId::Playlist(id) => project
                .playlists
                .get(id)
                .map(|playlist| &playlist.end_action),
            NodeId::ChapterPlaylist(id) => project
                .chapter_playlists
                .get(id)
                .map(|playlist| &playlist.end_action),
            NodeId::Menu(id) => project.menus.get(id).map(|menu| &menu.end_action),
            NodeId::FirstPlay => None,
        },
    }
}

/// Resolve a link to an existing graph node.
fn resolve_target(project: &Project, link: &Link) -> Option<NodeId> {
    match link {
        Link::Timeline { id, .. } => project
            .timelines
            .contains_key(id)
            .then_some(NodeId::Timeline(*id)),
        Link::Menu { id, .. } => project.menus.contains_key(id).then_some(NodeId::Menu(*id)),
        Link::Playlist { id } => project
            .playlists
            .contains_key(id)
            .then_some(NodeId::Playlist(*id)),
        Link::Slideshow { id } => project
            .slideshows
            .contains_key(id)
            .then_some(NodeId::Slideshow(*id)),
        Link::ChapterPlaylist { id } => project
            .chapter_playlists
            .contains_key(id)
            .then_some(NodeId::ChapterPlaylist(*id)),
        Link::NotSet | Link::Stop | Link::LastMenu => None,
    }
}

/// Text for a port row.
fn port_text(project: &Project, label: &str, link: &Link, target: Option<NodeId>) -> String {
    if target.is_some() {
        return label.to_owned();
    }

    tlf!(
        "{label} → {target}",
        label = label,
        target = link_label(project, link)
    )
}

/// Label drawn on an edge.
fn edge_label(link: &Link) -> String {
    match link {
        Link::Timeline { chapter, .. } if *chapter > 0 => {
            tlf!("Chapter {n}", n = (*chapter).saturating_add(1))
        }
        _ => String::new(),
    }
}

/// Use an object name when present, otherwise a generated fallback.
fn object_title(name: &str, fallback: String) -> String {
    if name.is_empty() {
        fallback
    } else {
        name.to_owned()
    }
}

/// Column index for a node kind.
fn column_of(id: NodeId) -> usize {
    match id {
        NodeId::FirstPlay => 0,
        NodeId::Menu(_) => 1,
        NodeId::Timeline(_) | NodeId::Slideshow(_) => 2,
        NodeId::Playlist(_) | NodeId::ChapterPlaylist(_) => 3,
    }
}

/// X coordinate for a column.
fn column_x(origin: Pos2, column: usize) -> f32 {
    let step = NODE_WIDTH + COLUMN_GAP;
    match column {
        0 => origin.x,
        1 => origin.x + step,
        2 => origin.x + step * 2.0,
        _ => origin.x + step * 3.0,
    }
}

/// Height of a node from its port count.
fn node_height(node: &Node) -> f32 {
    let rows = u16::try_from(node.ports.len()).unwrap_or(u16::MAX);
    HEADER + thumb_height(node) + f32::from(rows) * ROW
}

/// Height of the thumbnail band (0 when the node has no media).
fn thumb_height(node: &Node) -> f32 {
    if node.media.is_some() { THUMB } else { 0.0 }
}

/// Y coordinate of a port row (below the header and the thumbnail band).
fn port_y(rect: Rect, node: &Node, index: usize) -> f32 {
    let row = u16::try_from(index).unwrap_or(u16::MAX);
    rect.min.y + HEADER + thumb_height(node) + (f32::from(row) + 0.5) * ROW
}

/// Thumbnail rectangle of a node (between the header and the ports).
fn thumb_rect(rect: Rect) -> Rect {
    Rect::from_min_size(
        Pos2::new(rect.min.x + PAD, rect.min.y + HEADER + 4.0),
        vec2(rect.width() - PAD * 2.0, THUMB - 8.0),
    )
}

/// Header rectangle of a node.
fn header_rect(rect: Rect) -> Rect {
    Rect::from_min_size(rect.min, vec2(rect.width(), HEADER))
}

/// Size needed by a laid-out graph.
fn content_size(rects: &[(NodeId, Rect)]) -> Vec2 {
    if rects.is_empty() {
        return vec2(NODE_WIDTH + PADDING, HEADER + PADDING);
    }

    let mut max_x: f32 = 0.0;
    let mut max_y: f32 = 0.0;

    for (_, rect) in rects {
        max_x = max_x.max(rect.max.x);
        max_y = max_y.max(rect.max.y);
    }

    // Room for the lanes of back links below the nodes.
    vec2(max_x + PADDING, max_y + PADDING + BACK_LANE_GAP * 9.0)
}

/// Draw all nodes.
fn draw_nodes(
    painter: &Painter,
    ui: &Ui,
    nodes: &[Node],
    rects: &[(NodeId, Rect)],
    selection: Selection,
    thumbs: &BTreeMap<u64, TextureHandle>,
) {
    let visuals = ui.visuals();

    for (id, rect) in rects {
        let Some(node) = nodes.iter().find(|node| node.id == *id) else {
            continue;
        };
        let selected = node_selected(selection, *id);
        draw_node(painter, visuals, node, *rect, selected);
        if let Some(asset) = node.media {
            draw_thumb(painter, visuals, *rect, thumbs.get(&asset.0));
        }
    }
}

/// Draw one node.
fn draw_node(painter: &Painter, visuals: &Visuals, node: &Node, rect: Rect, selected: bool) {
    let stroke = if selected {
        visuals.selection.stroke
    } else {
        visuals.widgets.inactive.fg_stroke
    };

    painter.rect_filled(rect, RADIUS, visuals.widgets.inactive.bg_fill);

    let header = header_rect(rect);
    painter.rect_filled(header, RADIUS, visuals.selection.bg_fill);
    painter.rect_stroke(rect, RADIUS, stroke, StrokeKind::Inside);

    let text_color = visuals.text_color();
    painter.text(
        header.center(),
        Align2::CENTER_CENTER,
        &node.title,
        FontId::proportional(13.0),
        text_color,
    );

    for (index, (_, text)) in node.ports.iter().enumerate() {
        let y = port_y(rect, node, index);
        let center = Pos2::new(rect.max.x, y);

        // Long port texts (「エンドアクション → 前のメニューに戻る」) are cut at the node edge with ….
        let max_width = rect.width() - PAD * 2.0 - PORT_RADIUS;
        draw_elided(
            painter,
            Pos2::new(rect.min.x + PAD, y),
            text,
            max_width,
            text_color,
        );

        let circle = Rect::from_min_size(
            center - vec2(PORT_RADIUS, PORT_RADIUS),
            vec2(PORT_RADIUS * 2.0, PORT_RADIUS * 2.0),
        );
        painter.rect_filled(circle, PORT_RADIUS, stroke.color);
    }
}

/// One line of 12 px text, vertically centred on `left_center.y`, cut with … when wider than `max_width`.
fn draw_elided(
    painter: &Painter,
    left_center: Pos2,
    text: &str,
    max_width: f32,
    color: egui::Color32,
) {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        FontId::proportional(12.0),
        color,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(0.0));
    let galley = painter.layout_job(job);
    let top = left_center.y - galley.size().y / 2.0;
    painter.galley(Pos2::new(left_center.x, top), galley, color);
}

/// The thumbnail band of a node: the image fitted in a dark frame, or an empty frame while none is available.
fn draw_thumb(painter: &Painter, visuals: &Visuals, rect: Rect, texture: Option<&TextureHandle>) {
    let frame = thumb_rect(rect);
    painter.rect_filled(frame, 2.0, visuals.extreme_bg_color);
    if let Some(texture) = texture {
        crate::project_panel::draw_fitted(painter, frame, texture);
    }
}

/// Draw all edges.
fn draw_edges(
    painter: &Painter,
    ui: &Ui,
    edges: &[Edge],
    centers: &[(Port, Pos2)],
    rects: &[(NodeId, Rect)],
) {
    let color = ui.visuals().widgets.inactive.fg_stroke.color;
    let bottom = rects
        .iter()
        .map(|(_, rect)| rect.max.y)
        .fold(0.0_f32, f32::max);

    for (index, edge) in edges.iter().enumerate() {
        let Some((_, from)) = centers.iter().find(|(port, _)| *port == edge.from) else {
            continue;
        };
        let from = *from;

        let Some((_, target)) = rects.iter().find(|(id, _)| *id == edge.to) else {
            continue;
        };
        let target = *target;

        let to = Pos2::new(target.min.x, target.center().y);
        // Back links (to a column on the left) go around below the nodes; each gets its own lane.
        let lane = bottom
            + BACK_LANE_GAP * (1.0 + f32::from(u16::try_from(index).unwrap_or(u16::MAX) % 8));
        let path = edge_path(from, to, lane);
        let stroke = Stroke::new(1.0, color);

        for pair in path.windows(2) {
            if let [a, b] = pair {
                painter.line_segment([*a, *b], stroke);
            }
        }
        if let [.., before, tip] = path.as_slice() {
            arrow_head(painter, *before, *tip, stroke);
        }

        if !edge.label.is_empty() {
            let mid = Pos2::new((from.x + to.x) * 0.5, (from.y + to.y) * 0.5);
            painter.text(
                mid,
                Align2::CENTER_CENTER,
                &edge.label,
                FontId::proportional(11.0),
                color,
            );
        }
    }
}

/// Vertical distance between the lanes of back links below the nodes.
const BACK_LANE_GAP: f32 = 10.0;
/// Horizontal run out of a port / into a node before a back link turns.
const BACK_STUB: f32 = 12.0;
/// Length of an arrow head.
const ARROW_HEAD: f32 = 8.0;

/// The polyline of a link: straight when the target is to the right, otherwise out to the right, down to `lane`,
/// left past the target and up into its left edge.
pub(crate) fn edge_path(from: Pos2, to: Pos2, lane: f32) -> Vec<Pos2> {
    if to.x > from.x + BACK_STUB {
        return vec![from, to];
    }
    let out_x = from.x + BACK_STUB;
    let in_x = to.x - BACK_STUB;
    vec![
        from,
        Pos2::new(out_x, from.y),
        Pos2::new(out_x, lane),
        Pos2::new(in_x, lane),
        Pos2::new(in_x, to.y),
        to,
    ]
}

/// A fixed-size arrow head at `tip`, pointing from `before` to `tip`.
fn arrow_head(painter: &Painter, before: Pos2, tip: Pos2, stroke: Stroke) {
    let direction = (tip - before).normalized();
    if !direction.x.is_finite() || !direction.y.is_finite() {
        return;
    }
    let back = -direction * ARROW_HEAD;
    let side = Vec2::new(-direction.y, direction.x) * (ARROW_HEAD * 0.5);
    painter.line_segment([tip, tip + back + side], stroke);
    painter.line_segment([tip, tip + back - side], stroke);
}

/// Screen positions of all port circles.
fn port_centers(nodes: &[Node], rects: &[(NodeId, Rect)]) -> Vec<(Port, Pos2)> {
    let mut centers = Vec::new();

    for (id, rect) in rects {
        let Some(node) = nodes.iter().find(|node| node.id == *id) else {
            continue;
        };

        for (index, (port, _)) in node.ports.iter().enumerate() {
            centers.push((*port, Pos2::new(rect.max.x, port_y(*rect, node, index))));
        }
    }

    centers
}

/// Whether a node matches the current selection.
fn node_selected(selection: Selection, id: NodeId) -> bool {
    match (selection, id) {
        (Selection::Menu(a), NodeId::Menu(b)) => a == b,
        (Selection::Timeline(a), NodeId::Timeline(b)) => a == b,
        (Selection::Slideshow(a), NodeId::Slideshow(b)) => a == b,
        (Selection::Playlist(a), NodeId::Playlist(b)) => a == b,
        (Selection::ChapterPlaylist(a), NodeId::ChapterPlaylist(b)) => a == b,
        _ => false,
    }
}

/// Temporary-memory id for the flowchart drag state.
fn flow_drag_id(_ui: &Ui) -> Id {
    // One flowchart exists at a time, so a globally unique id is enough.
    Id::unique("disccraft_flowchart_drag")
}

/// Read the current drag state from egui temp memory.
fn flow_drag_state(ui: &Ui) -> Option<FlowDrag> {
    let id = flow_drag_id(ui);
    ui.data(|data| data.get_temp::<FlowDrag>(id))
}

/// Start a drag if the pointer pressed a port circle.
fn begin_flow_drag(
    ui: &mut Ui,
    response: &Response,
    centers: &[(Port, Pos2)],
    drag: Option<FlowDrag>,
) -> Option<FlowDrag> {
    if !response.drag_started() {
        return drag;
    }

    // A drag starts only after the pointer has moved past egui's threshold, so hit-test the port where the
    // button was pressed, not where the pointer is now (a quick drag would otherwise miss the port).
    let pointer = ui
        .input(|input| input.pointer.press_origin())
        .or_else(|| response.interact_pointer_pos());
    let state = start_drag(pointer, centers);
    let id = flow_drag_id(ui);

    ui.data_mut(|data| {
        if let Some(state) = state {
            data.insert_temp(id, state);
        } else {
            data.remove::<FlowDrag>(id);
        }
    });

    state
}

/// Finish a drag when the pointer is released.
fn finish_flow_drag(
    app: &mut DiscCraftApp,
    ui: &mut Ui,
    response: &Response,
    drag: Option<FlowDrag>,
    rects: &[(NodeId, Rect)],
) -> Option<FlowDrag> {
    if !response.drag_stopped() {
        return drag;
    }

    if let Some(state) = drag {
        let pointer = response
            .interact_pointer_pos()
            .or_else(|| ui.ctx().pointer_hover_pos());
        finish_drag(app, pointer, state, rects);
    }

    let id = flow_drag_id(ui);
    ui.data_mut(|data| {
        data.remove::<FlowDrag>(id);
    });

    None
}

/// Clear a stale drag if the pointer is no longer down.
fn clear_stale_flow_drag(ui: &mut Ui, drag: Option<FlowDrag>) -> Option<FlowDrag> {
    if drag.is_none() || ui.input(|input| input.pointer.any_down()) {
        return drag;
    }

    let id = flow_drag_id(ui);
    ui.data_mut(|data| {
        data.remove::<FlowDrag>(id);
    });

    None
}

/// Hit-test a port circle.
fn start_drag(pointer: Option<Pos2>, centers: &[(Port, Pos2)]) -> Option<FlowDrag> {
    let point = pointer?;
    let (port, _) = centers
        .iter()
        .find(|(_, center)| distance(point, *center) <= PORT_HIT)?;

    Some(FlowDrag { port: *port })
}

/// Apply the link represented by a drop on a node.
fn finish_drag(
    app: &mut DiscCraftApp,
    pointer: Option<Pos2>,
    drag: FlowDrag,
    rects: &[(NodeId, Rect)],
) {
    let Some(point) = pointer else {
        return;
    };
    let Some((target, _)) = rects.iter().find(|(_, rect)| rect.contains(point)) else {
        return;
    };
    if !port_accepts(&drag.port, *target) {
        return;
    }
    let Some(link) = link_to(*target) else {
        return;
    };
    let Some((command, base)) = command_for_port(&drag.port) else {
        return;
    };

    app.apply_link(command, base, &link);
}

/// Handle a click on a node header.
fn click_node(app: &mut DiscCraftApp, pointer: Option<Pos2>, rects: &[(NodeId, Rect)]) {
    let Some(point) = pointer else {
        return;
    };
    let Some((id, _)) = rects
        .iter()
        .find(|(_, rect)| header_rect(*rect).contains(point))
    else {
        return;
    };

    // Same as clicking the item in the Project panel: select it and bring its viewer forward.
    let selection = crate::context_menus::node_selection(*id);
    if selection != Selection::None {
        app.select(selection);
    }
}

/// Right-click menu of the flowchart: of the node under the pointer when the right button was clicked, or of the
/// background (Encore: すべてのサムネールを表示 / 新規 ▸ / 読み込み ▸ / プレビュー).
fn node_context_menu(
    app: &mut DiscCraftApp,
    ui: &Ui,
    response: &Response,
    rects: &[(NodeId, Rect)],
) {
    let key = Id::unique("flowchart_context_node");
    if response.secondary_clicked() {
        let node = response
            .interact_pointer_pos()
            .and_then(|point| rects.iter().find(|(_, rect)| rect.contains(point)))
            .map(|(id, _)| *id);
        ui.data_mut(|data| data.insert_temp(key, node));
        // Encore と同じく、右クリックでもそのノードを選ぶ。
        if let Some(node) = node {
            let selection = crate::context_menus::node_selection(node);
            if selection != Selection::None {
                app.select(selection);
            }
        }
    }

    let node = ui
        .data(|data| data.get_temp::<Option<NodeId>>(key))
        .flatten();
    let _ = response.context_menu(|ui| match node {
        Some(node) => app.flow_node_menu(ui, node),
        None => app.flow_background_menu(ui),
    });
}

/// Draw the temporary line while dragging a link.
fn draw_drag_line(
    painter: &Painter,
    ui: &Ui,
    centers: &[(Port, Pos2)],
    drag: FlowDrag,
    pointer: Option<Pos2>,
) {
    let Some(point) = pointer else {
        return;
    };
    let Some((_, start)) = centers.iter().find(|(port, _)| *port == drag.port) else {
        return;
    };

    let color = ui.visuals().selection.stroke.color;
    painter.line_segment([*start, point], Stroke::new(2.0, color));
}

/// Command id and base params for changing a port's link.
pub(crate) fn command_for_port(port: &Port) -> Option<(&'static str, Map<String, Value>)> {
    match port {
        Port::FirstPlay => Some(("nav.setFirstPlay", Map::new())),
        Port::TitleButton => Some(("nav.setTitleRemote", Map::new())),
        Port::Button { menu, index } => {
            let button = u64::try_from(*index).ok()?;
            let mut map = Map::new();
            let _ = map.insert("menu".to_owned(), Value::from(menu.0));
            let _ = map.insert("button".to_owned(), Value::from(button));
            Some(("button.setLink", map))
        }
        Port::EndAction(source) => end_action_command(source),
        Port::MenuRemote(id) => {
            let mut map = Map::new();
            let _ = map.insert("timeline".to_owned(), Value::from(id.0));
            Some(("timeline.setMenuRemote", map))
        }
        Port::Override(id) => {
            let mut map = Map::new();
            let _ = map.insert("timeline".to_owned(), Value::from(id.0));
            Some(("timeline.setOverride", map))
        }
        Port::MenuOverride(id) => {
            let mut map = Map::new();
            let _ = map.insert("menu".to_owned(), Value::from(id.0));
            Some(("menu.setOverride", map))
        }
        Port::DiscOverride => Some(("nav.setFirstPlayOverride", Map::new())),
        Port::ChapterPlaylistOverride(id) => {
            let mut map = Map::new();
            let _ = map.insert("chapterPlaylist".to_owned(), Value::from(*id));
            Some(("chapterplaylist.setOverride", map))
        }
        Port::PlaylistItems(id) => {
            let mut map = Map::new();
            let _ = map.insert("playlist".to_owned(), Value::from(*id));
            Some(("playlist.addItem", map))
        }
        Port::ButtonOverride { menu, index } => {
            let button = u64::try_from(*index).ok()?;
            let mut map = Map::new();
            let _ = map.insert("menu".to_owned(), Value::from(menu.0));
            let _ = map.insert("button".to_owned(), Value::from(button));
            Some(("button.setOverride", map))
        }
    }
}

/// Command id and base params for an end-action port.
fn end_action_command(source: &NodeId) -> Option<(&'static str, Map<String, Value>)> {
    let mut map = Map::new();

    match source {
        NodeId::Timeline(id) => {
            let _ = map.insert("timeline".to_owned(), Value::from(id.0));
            Some(("timeline.setEndAction", map))
        }
        NodeId::Slideshow(id) => {
            let _ = map.insert("slideshow".to_owned(), Value::from(*id));
            Some(("slideshow.setEndAction", map))
        }
        NodeId::Playlist(id) => {
            let _ = map.insert("playlist".to_owned(), Value::from(*id));
            Some(("playlist.setEndAction", map))
        }
        NodeId::ChapterPlaylist(id) => {
            let _ = map.insert("chapterPlaylist".to_owned(), Value::from(*id));
            Some(("chapterplaylist.setEndAction", map))
        }
        NodeId::Menu(id) => {
            let _ = map.insert("menu".to_owned(), Value::from(id.0));
            Some(("menu.setEndAction", map))
        }
        NodeId::FirstPlay => None,
    }
}

/// Euclidean distance between two points.
fn distance(a: Pos2, b: Pos2) -> f32 {
    (a - b).length()
}

impl DiscCraftApp {
    /// Draw the flowchart and handle clicks and link dragging.
    pub(crate) fn flowchart_view(&mut self, ui: &mut Ui) {
        ui.label(tl!(
            "Drag from a button or an end action to a menu, timeline, slideshow or playlist to link it."
        ));

        let _ = ScrollArea::both().show(ui, |ui| {
            self.flowchart_canvas(ui);
        });
    }

    /// Draw one flowchart canvas and process its interaction.
    fn flowchart_canvas(&mut self, ui: &mut Ui) {
        let (mut nodes, edges) = build_graph(&self.session.project);
        // サムネールを出さないときはノードが絵の分だけ低くなる（Encore と同じ）。
        if !self.flow_thumbnails {
            for node in &mut nodes {
                node.media = None;
            }
        }
        // 「ボタンを表示」を外したメニューはボタンのポートを畳む。リンクの線はノードの右端から出す。
        let hidden_buttons = self.flow_hidden_buttons.clone();
        if !hidden_buttons.is_empty() {
            for node in &mut nodes {
                if let NodeId::Menu(id) = node.id
                    && hidden_buttons.contains(&id)
                {
                    node.ports.clear();
                }
            }
        }
        let (preview, _) = layout_split(&nodes, &edges, Pos2::ZERO);
        let size = content_size(&preview);

        let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
        let (rects, divider) = layout_split(&nodes, &edges, response.rect.min);
        let mut centers = port_centers(&nodes, &rects);
        for edge in &edges {
            if let Port::Button { menu, .. } = edge.from
                && hidden_buttons.contains(&menu)
                && let Some((_, rect)) = rects.iter().find(|(id, _)| *id == NodeId::Menu(menu))
            {
                centers.push((edge.from, Pos2::new(rect.max.x, rect.center().y)));
            }
        }

        if let Some(y) = divider {
            draw_unlinked_heading(&painter, ui, response.rect, y);
        }

        let thumbs = self.node_thumbnails(ui.ctx(), &nodes);
        draw_nodes(&painter, ui, &nodes, &rects, self.selection, &thumbs);
        draw_edges(&painter, ui, &edges, &centers, &rects);

        let mut drag = flow_drag_state(ui);
        drag = begin_flow_drag(ui, &response, &centers, drag);
        drag = finish_flow_drag(self, ui, &response, drag, &rects);
        drag = clear_stale_flow_drag(ui, drag);

        if let Some(state) = drag {
            draw_drag_line(&painter, ui, &centers, state, ui.ctx().pointer_hover_pos());
        }

        if response.clicked() {
            click_node(self, response.interact_pointer_pos(), &rects);
        }
        node_context_menu(self, ui, &response, &rects);
    }

    /// Thumbnails of the nodes' media (cached per asset by `asset_thumbnail`).
    fn node_thumbnails(
        &mut self,
        ctx: &egui::Context,
        nodes: &[Node],
    ) -> BTreeMap<u64, TextureHandle> {
        let mut thumbs = BTreeMap::new();
        for asset in nodes.iter().filter_map(|node| node.media) {
            if let Some(texture) = self.asset_thumbnail(ctx, asset) {
                let _ = thumbs.insert(asset.0, texture);
            }
        }
        thumbs
    }

    /// Run a link-changing command with a target.
    pub(crate) fn apply_link(&mut self, command: &str, base: Map<String, Value>, link: &Link) {
        match with_target(base, link) {
            Ok(params) => {
                let _ = self.run_command(command, params);
            }
            Err(error) => self.status = error,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the flowchart graph and layout.

    use super::{
        Node, NodeId, Port, build_graph, column_of, command_for_port, edge_path, layout,
        layout_split, link_to, port_accepts,
    };
    use disccraft_project::{Link, MenuId, Project, TimelineId};
    use egui::Pos2;
    use std::path::Path;

    /// Build a small project and check graph nodes and edges.
    #[test]
    fn build_graph_links_and_ports() -> Result<(), String> {
        let mut project = Project::default();
        let asset = project
            .add_asset(Path::new("a.m2v"))
            .map_err(|error| error.to_string())?;
        let timeline = project
            .add_timeline(asset, None)
            .map_err(|error| error.to_string())?;
        let menu = project.add_menu(None).map_err(|error| error.to_string())?;

        let menu_obj = project
            .menus
            .get_mut(&menu)
            .ok_or_else(|| "missing menu".to_owned())?;
        menu_obj.buttons.clear();
        project
            .add_button(menu, "Play".to_owned(), None, Link::NotSet)
            .map_err(|error| error.to_string())?;
        project
            .add_button(menu, "Menu".to_owned(), None, Link::NotSet)
            .map_err(|error| error.to_string())?;
        let menu_obj = project
            .menus
            .get_mut(&menu)
            .ok_or_else(|| "missing menu".to_owned())?;

        let first_button = menu_obj
            .buttons
            .get_mut(0)
            .ok_or_else(|| "missing first button".to_owned())?;
        first_button.label = "Play".to_owned();
        first_button.link = Link::Timeline {
            id: timeline,
            chapter: 2,
        };

        let second_button = menu_obj
            .buttons
            .get_mut(1)
            .ok_or_else(|| "missing second button".to_owned())?;
        second_button.label = "Menu".to_owned();
        second_button.link = Link::Menu {
            id: menu,
            button: None,
        };

        let timeline_obj = project
            .timelines
            .get_mut(&timeline)
            .ok_or_else(|| "missing timeline".to_owned())?;
        timeline_obj.end_action = Link::LastMenu;

        project.settings.first_play = Link::Menu {
            id: menu,
            button: None,
        };

        let (nodes, edges) = build_graph(&project);

        assert_eq!(nodes.len(), 3);

        let first = nodes
            .iter()
            .find(|node| node.id == NodeId::FirstPlay)
            .ok_or_else(|| "missing first play".to_owned())?;
        // First Play and the Title button.
        assert_eq!(first.ports.len(), 2);

        let menu_node = nodes
            .iter()
            .find(|node| node.id == NodeId::Menu(menu))
            .ok_or_else(|| "missing menu node".to_owned())?;
        assert_eq!(menu_node.ports.len(), 2);

        let timeline_node = nodes
            .iter()
            .find(|node| node.id == NodeId::Timeline(timeline))
            .ok_or_else(|| "missing timeline node".to_owned())?;
        assert_eq!(timeline_node.ports.len(), 1);

        assert_eq!(edges.len(), 3);
        assert!(
            edges
                .iter()
                .any(|edge| edge.from == Port::FirstPlay && edge.to == NodeId::Menu(menu))
        );
        assert!(
            edges
                .iter()
                .any(|edge| edge.from == Port::Button { menu, index: 0 }
                    && edge.to == NodeId::Timeline(timeline))
        );
        assert!(
            edges
                .iter()
                .any(|edge| edge.from == Port::Button { menu, index: 1 }
                    && edge.to == NodeId::Menu(menu))
        );
        assert!(
            !edges
                .iter()
                .any(|edge| edge.from == Port::EndAction(NodeId::Timeline(timeline)))
        );

        Ok(())
    }

    /// Layout places columns left to right without overlaps.
    #[test]
    fn layout_columns_do_not_overlap() {
        let nodes = vec![
            Node {
                id: NodeId::FirstPlay,
                title: "First".to_owned(),
                ports: vec![(Port::FirstPlay, "p".to_owned())],
                media: None,
            },
            Node {
                id: NodeId::Menu(MenuId(1)),
                title: "Menu".to_owned(),
                ports: vec![(
                    Port::Button {
                        menu: MenuId(1),
                        index: 0,
                    },
                    "b".to_owned(),
                )],
                media: None,
            },
            Node {
                id: NodeId::Timeline(TimelineId(1)),
                title: "T".to_owned(),
                ports: vec![(
                    Port::EndAction(NodeId::Timeline(TimelineId(1))),
                    "e".to_owned(),
                )],
                media: None,
            },
            Node {
                id: NodeId::Playlist(1),
                title: "P".to_owned(),
                ports: vec![(Port::EndAction(NodeId::Playlist(1)), "e".to_owned())],
                media: None,
            },
        ];

        let rects = layout(&nodes, Pos2::ZERO);
        assert_eq!(rects.len(), 4);

        for (i, (id_a, rect_a)) in rects.iter().enumerate() {
            for (id_b, rect_b) in rects.iter().skip(i + 1) {
                let same_column = column_of(*id_a) == column_of(*id_b);

                if same_column {
                    assert!(rect_a.max.y <= rect_b.min.y || rect_b.max.y <= rect_a.min.y);
                } else {
                    assert!(rect_a.max.x <= rect_b.min.x || rect_b.max.x <= rect_a.min.x);
                }
            }
        }
    }

    /// Items nothing links to go to a row below the linked graph.
    #[test]
    fn unlinked_items_go_below() -> Result<(), String> {
        let mut project = Project::default();
        let linked = project
            .add_menu(Some("top".to_owned()))
            .map_err(|error| error.to_string())?;
        let lonely = project
            .add_menu(Some("lonely".to_owned()))
            .map_err(|error| error.to_string())?;
        project.settings.first_play = Link::Menu {
            id: linked,
            button: None,
        };

        let (nodes, edges) = build_graph(&project);
        let (rects, divider) = layout_split(&nodes, &edges, Pos2::ZERO);
        let divider = divider.ok_or_else(|| "expected an unlinked row".to_owned())?;
        let rect_of = |id: NodeId| {
            rects
                .iter()
                .find(|(node, _)| *node == id)
                .map(|(_, rect)| *rect)
                .ok_or_else(|| format!("missing {id:?}"))
        };
        assert!(rect_of(NodeId::Menu(linked))?.max.y < divider);
        assert!(rect_of(NodeId::Menu(lonely))?.min.y > divider);
        Ok(())
    }

    /// The Title button links only to menus and uses its own command.
    #[test]
    fn title_button_port_accepts_menus_only() {
        assert!(port_accepts(&Port::TitleButton, NodeId::Menu(MenuId(1))));
        assert!(!port_accepts(
            &Port::TitleButton,
            NodeId::Timeline(TimelineId(1))
        ));
        assert!(!port_accepts(&Port::TitleButton, NodeId::FirstPlay));
        assert!(port_accepts(
            &Port::FirstPlay,
            NodeId::Timeline(TimelineId(1))
        ));
        assert_eq!(
            command_for_port(&Port::TitleButton).map(|(command, _)| command),
            Some("nav.setTitleRemote")
        );
    }

    /// Each node kind maps to the expected link.
    #[test]
    fn link_to_maps_nodes() {
        assert!(link_to(NodeId::FirstPlay).is_none());
        assert_eq!(
            link_to(NodeId::Menu(MenuId(7))),
            Some(Link::Menu {
                id: MenuId(7),
                button: None
            })
        );
        assert_eq!(
            link_to(NodeId::Timeline(TimelineId(3))),
            Some(Link::Timeline {
                id: TimelineId(3),
                chapter: 0
            })
        );
        assert_eq!(
            link_to(NodeId::Slideshow(5)),
            Some(Link::Slideshow { id: 5 })
        );
        assert_eq!(link_to(NodeId::Playlist(9)), Some(Link::Playlist { id: 9 }));
        assert_eq!(
            link_to(NodeId::ChapterPlaylist(4)),
            Some(Link::ChapterPlaylist { id: 4 })
        );
    }

    #[test]
    fn forward_links_are_straight_and_back_links_go_below() {
        let forward = edge_path(Pos2::new(100.0, 50.0), Pos2::new(300.0, 80.0), 400.0);
        assert_eq!(forward.len(), 2);

        let back = edge_path(Pos2::new(500.0, 50.0), Pos2::new(200.0, 80.0), 400.0);
        assert_eq!(back.len(), 6);
        assert_eq!(back.first(), Some(&Pos2::new(500.0, 50.0)));
        assert_eq!(back.last(), Some(&Pos2::new(200.0, 80.0)));
        // Every bend stays at or below the lane between leaving the port and entering the node.
        assert!(
            back.iter()
                .skip(2)
                .take(2)
                .all(|point| (point.y - 400.0).abs() < f32::EPSILON)
        );
    }
}
