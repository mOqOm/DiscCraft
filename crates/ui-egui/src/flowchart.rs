//! The flowchart of the disc's navigation, laid out and drawn as Encore's (M6.432, measured on Encore CS6): a tree
//! from the disc to the right in which every link leads to a node of its own (a link to a node already in the tree
//! to a faint repeat of it), each node lined up with the button or end action its link leaves from, and the items
//! the disc never reaches in a tray below, under a zoom slider.
//!
//! A node is its picture with the mark of its kind at the top left and, at the top right, where its link goes in
//! (Encore: "A2 S1 C1" for the audio and subtitle the link selects and the chapter it plays from, "B2" for the button
//! a menu is entered at), over its name; a menu lists its buttons under it page by page. The arrow of a link is
//! two thirds along it. The pictures, marks and the disc go when thumbnails are off.

use crate::app::{DiscCraftApp, Selection};
use crate::fonts::chart_font;
use crate::gui_params::with_target;
use crate::i18n::{tl, tlf};
use crate::menu_editor::link_label;
use crate::project_panel::{RowIcon, draw_fitted_tinted, paint_glyph};
use crate::scroll_bars::{Axis, BAR, Span, scroll_bar};
use disccraft_project::{
    AssetId, Button, FLOWCHART_DISC, FLOWCHART_LIMIT, FlowPoint, Link, Menu, MenuId, PlaylistItem,
    Project, SubtitleDefault, Tick, TimelineId,
};
use egui::{
    Color32, CornerRadius, Id, Painter, Pos2, Rect, Response, ScrollArea, Sense, Shape, Stroke,
    StrokeKind, TextureHandle, Ui, Vec2, vec2,
};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Width of a node at zoom 1 (Encore's, as its flowchart shows them).
const NODE_WIDTH: f32 = 86.0;
/// Height of a node's picture with the edge above it (Encore: 2 + 59).
const THUMB: f32 = 61.0;
/// Height of a node's name bar with the line above it and the edge below it (Encore: 1 + 17 + 1).
const NAME_BAR: f32 = 19.0;
/// Height of a menu's button row and page heading.
const ROW: f32 = 17.0;
/// Height of the band between a menu's name and its pages (Encore's starts on the name's lower edge).
const ACCENT: f32 = 8.0;
/// Width of the edge around a node's picture (its name has one of [`EDGE`]).
const RIM: f32 = 2.0;
/// Width of the edge around a node's name.
const EDGE: f32 = 1.0;
/// Size of the disc's picture.
const DISC: f32 = 72.0;
/// Width of the disc node (its name's box).
const DISC_WIDTH: f32 = 86.0;
/// Height of the disc's name box.
const DISC_LABEL: f32 = 19.0;
/// Room above the tree.
const MARGIN: f32 = 16.0;
/// Room left of the tree (Encore: the disc's name 12 from the flowchart's edge).
const MARGIN_LEFT: f32 = 12.0;
/// Vertical gap between the nodes of one column.
const GAP: f32 = 17.0;
/// Horizontal gap between columns.
const COLUMN_GAP: f32 = 94.0;
/// Room right of and below the last nodes.
const PADDING: f32 = 24.0;
/// Inner padding of texts.
const PAD: f32 = 4.0;
/// Length of an arrow head (Encore's: 12 long, 9 wide, its back almost straight).
const ARROW_HEAD: f32 = 12.0;
/// How far along its link an arrow's tip is (Encore's are past the middle).
const ARROW_AT: f32 = 0.69;
/// Size of the texts: names, rows, headings and marks (Encore: all Tahoma at 11 pixels, see
/// [`crate::fonts::chart_font`]).
const TEXT_SIZE: f32 = 11.0;
/// Gap between the tray's items.
const TRAY_GAP: f32 = 14.0;
/// Room in the tray above, below and left of its items (Encore: 17, 20 and 7).
const TRAY_TOP: f32 = 17.0;
const TRAY_BOTTOM: f32 = 20.0;
const TRAY_LEFT: f32 = 7.0;
/// The smallest zoom of the slider (Encore: 10 %).
pub(crate) const MIN_ZOOM: f32 = 0.1;
/// The largest zoom of the slider: Encore's flowchart opens at its largest, 100 %, and only zooms out.
pub(crate) const MAX_ZOOM: f32 = 1.0;
/// How much a click on the slider's small or large mark zooms (Encore: 10 % a click).
const ZOOM_STEP: f32 = 0.1;
/// Hit radius around a port.
const PORT_HIT: f32 = 8.0;
/// Width of the right end of a name where its end action's link is dragged from.
const END_GRIP: f32 = 14.0;
/// How far a repeat's picture and edge are faded into the ground (Encore: halfway, white is 160).
const REPEAT_FADE: f32 = 0.5;
/// Width of the glow around the selected node (Encore: 9 pixels from its colour at the edge to the ground).
const GLOW_WIDTH: f32 = 9.0;
/// How far the vertical scroll bar is in from the panel's right and top (Encore: 3).
const BAR_INSET: f32 = 3.0;
/// Width of the zoom controls at the left of the row under the flowchart (Encore: 187).
const ZOOM_WIDTH: f32 = 187.0;
/// Width of the zoom slider's thumb (Encore: 11).
const ZOOM_THUMB: f32 = 11.0;

/// The flowchart's ground.
const GROUND: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(64), Color32::from_gray(242));
/// The tray's ground.
const TRAY: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(64), Color32::from_gray(242));
/// A picture's ground (and the letterbox of a picture of another shape).
const PICTURE_FILL: Color32 = Color32::from_gray(0);
/// A node without a picture (an empty slideshow or playlist).
const NO_PICTURE: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(127), Color32::from_gray(190));
/// The top and bottom of a name bar and a page heading: shaded from 82 to 58 down their 17 pixels (Encore).
const NAME_TOP: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(83), Color32::from_gray(235));
const NAME_BOTTOM: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(57), Color32::from_gray(205));
/// A menu's button rows.
const ROWS_FILL: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(89), Color32::from_gray(226));
/// The selected button's row: darker than the others, its text as it is (Encore).
const ROW_LIT: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(68), Color32::from_gray(200));
/// The top and bottom of the band between a menu's name and its pages: (230, 185, 161) down to (205, 118, 70).
const ACCENT_TOP: Color32 = Color32::from_rgb(232, 190, 168);
const ACCENT_BOTTOM: Color32 = Color32::from_rgb(203, 113, 64);
/// The glow around the selected node at its edge (fading to the ground over [`GLOW_WIDTH`]), and around a repeat of
/// it.
const GLOW: crate::theme::Shade = crate::theme::Shade::new(
    Color32::from_rgb(92, 98, 113),
    Color32::from_rgb(170, 180, 205),
);
const GLOW_REPEAT: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(82), Color32::from_gray(200));
/// The backing of a kind's mark on a picture.
const MARK_FILL: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 150);
/// The backing of a link's marks: half-transparent white over the picture (Encore).
const BADGE_FILL: Color32 = Color32::from_rgba_premultiplied(128, 128, 128, 128);
/// The text of a link's marks.
const BADGE_TEXT: Color32 = Color32::BLACK;
/// Text of rows.
const TEXT: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(215), Color32::from_gray(40));
/// Names and page headings.
const TEXT_STRONG: crate::theme::Shade =
    crate::theme::Shade::new(Color32::WHITE, Color32::from_gray(0));
/// A repeat's name.
const TEXT_REPEAT: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(175), Color32::from_gray(100));
/// Links and arrows.
const LINE: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(188), Color32::from_gray(90));
/// The selected button's link.
const LINE_LIT: Color32 = Color32::from_rgb(0, 83, 227);
/// The edge of the node a drop would link (Encore).
pub(crate) const DROP_EDGE: Color32 = Color32::from_rgb(199, 138, 17);
/// The dark lines above and below the row under the flowchart, and around the zoom slider's groove (Encore: 6).
const ROW_LINE: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(6), Color32::from_gray(160));
/// The zoom slider's groove and the light line under it (Encore: 45 and 66).
const GROOVE: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(45), Color32::from_gray(200));
const GROOVE_LIGHT: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(66), Color32::from_gray(252));
/// The zoom slider's thumb: its edge and face (Encore: 13, and 87 down to 65).
const ZOOM_THUMB_EDGE: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(13), Color32::from_gray(120));
const ZOOM_THUMB_FACE: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(76), Color32::from_gray(230));
/// The zoom marks, and a mark at its end (Encore: 188 and 87).
const ZOOM_MARK: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(188), Color32::from_gray(90));
const ZOOM_MARK_DIM: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(87), Color32::from_gray(190));
/// The disc's picture.
const DISC_FILL: Color32 = Color32::from_gray(186);
/// The disc's grooves.
const DISC_RING: Color32 = Color32::from_gray(150);
/// The disc's hub.
const DISC_HUB: Color32 = Color32::from_gray(222);
/// The border of the disc's name.
const DISC_EDGE: crate::theme::Shade =
    crate::theme::Shade::new(Color32::from_gray(35), Color32::from_gray(150));

/// The sizes of the flowchart at one zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Metrics {
    /// Width of a node.
    node_width: f32,
    /// Height of a picture.
    thumb: f32,
    /// Height of a name bar.
    name_bar: f32,
    /// Height of a row.
    row: f32,
    /// Height of a menu's band.
    accent: f32,
    /// Size of the disc's picture.
    disc: f32,
    /// Width of the disc node.
    disc_width: f32,
    /// Height of the disc's name box.
    disc_label: f32,
    /// Room left of and above the tree.
    margin: f32,
    /// Room left of the tree.
    margin_left: f32,
    /// Gap between the nodes of a column.
    gap: f32,
    /// Gap between columns.
    column_gap: f32,
    /// Inner padding of texts.
    pad: f32,
    /// Length of an arrow head.
    arrow: f32,
    /// Size of the texts.
    text: f32,
    /// Width of the edge around a picture.
    rim: f32,
    /// Width of the edge around a name.
    edge: f32,
    /// Width of the selected node's glow.
    glow: f32,
    /// The zoom itself: a place a node was moved to is kept at zoom 1 and drawn this many times as far.
    zoom: f32,
}

impl Metrics {
    /// The sizes at `zoom` (kept between [`MIN_ZOOM`] and [`MAX_ZOOM`]; 1 for a zoom that is not a number).
    pub(crate) fn at(zoom: f32) -> Self {
        let z = if zoom.is_finite() {
            zoom.clamp(MIN_ZOOM, MAX_ZOOM)
        } else {
            1.0
        };
        Self {
            node_width: NODE_WIDTH * z,
            thumb: THUMB * z,
            name_bar: NAME_BAR * z,
            row: ROW * z,
            accent: ACCENT * z,
            disc: DISC * z,
            disc_width: DISC_WIDTH * z,
            disc_label: DISC_LABEL * z,
            margin: MARGIN * z,
            margin_left: MARGIN_LEFT * z,
            gap: GAP * z,
            column_gap: COLUMN_GAP * z,
            pad: PAD * z,
            arrow: ARROW_HEAD * z,
            text: TEXT_SIZE * z,
            rim: RIM * z,
            edge: EDGE * z,
            glow: GLOW_WIDTH * z,
            zoom: z,
        }
    }
}

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
    /// What plays in a timeline's place on a player below its parental level (M6.363).
    ParentalAlternative(TimelineId),
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
        | Port::ParentalAlternative(_)
        | Port::MenuOverride(_)
        | Port::DiscOverride
        | Port::ChapterPlaylistOverride(_)
        | Port::ButtonOverride { .. } => node != NodeId::FirstPlay,
    }
}

/// What a node shows as its thumbnail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Media {
    /// An asset's thumbnail (a slideshow's first slide).
    Asset(AssetId),
    /// A timeline's chapter, as its poster frame (Encore: a timeline shows its first chapter's poster, a chapter
    /// playlist its first entry's, a playlist its first item's).
    Chapter(TimelineId, u32),
    /// The menu's own picture, as the Project panel shows it (M6.408: Encore's flowchart shows a pop-up over the
    /// checkerboard with only its buttons, a menu of a plain colour with its texts, not the background's file).
    Menu(MenuId),
}

/// A flowchart node with its visible ports.
#[derive(Clone, Debug)]
pub(crate) struct Node {
    /// Graph node identity.
    pub(crate) id: NodeId,
    /// Its name.
    pub(crate) title: String,
    /// Ports with display text.
    pub(crate) ports: Vec<(Port, String)>,
    /// What its picture shows when its link does not say (a timeline's video, a slideshow's first slide, a menu's
    /// picture).
    pub(crate) media: Option<Media>,
    /// Where each page of a menu starts in `ports` (its rows are headed "Page 0", "Page 1"…). Empty for a node whose
    /// only port is its end action, on its name bar.
    pub(crate) pages: Vec<usize>,
    /// Whether its picture is drawn (Encore's すべてのサムネールを表示).
    pub(crate) picture: bool,
}

/// A navigation edge between a port and a target node.
#[derive(Clone, Debug)]
pub(crate) struct Edge {
    /// Source port.
    pub(crate) from: Port,
    /// The node the port is on.
    pub(crate) source: NodeId,
    /// Target node.
    pub(crate) to: NodeId,
    /// Where the link goes in, shown on the node it leads to (Encore: "A2 S1 C1", "B2").
    pub(crate) badge: String,
    /// The chapter of a timeline it plays from (the node it leads to shows that frame).
    pub(crate) chapter: Option<(TimelineId, u32)>,
    /// The button of a menu (its page) it goes in at (Encore: opening the node selects it).
    pub(crate) button: Option<(MenuId, u32)>,
}

/// Where the flowchart's nodes are.
#[derive(Clone, Debug, Default)]
pub(crate) struct Placement {
    /// The nodes in the tree, each once.
    pub(crate) rects: Vec<(NodeId, Rect)>,
    /// Faint repeats: the link (its index in the edges) that leads to one, the node it repeats, and where it is.
    pub(crate) repeats: Vec<(usize, NodeId, Rect)>,
    /// The link each node in the tree is reached by (not the disc).
    pub(crate) entered: BTreeMap<NodeId, usize>,
    /// The items the disc never reaches, for the tray, in its order.
    pub(crate) tray: Vec<NodeId>,
}

impl Placement {
    /// Where `id` is drawn in the tree.
    fn rect_of(&self, id: NodeId) -> Option<Rect> {
        self.rects
            .iter()
            .find(|(node, _)| *node == id)
            .map(|(_, rect)| *rect)
    }

    /// Where the link `index` (`edge`) leads: its repeat, or the node it reaches the tree by. `None` for a link of a
    /// tray item.
    fn target_of(&self, index: usize, edge: &Edge) -> Option<Rect> {
        if let Some((_, _, rect)) = self.repeats.iter().find(|(link, _, _)| *link == index) {
            return Some(*rect);
        }
        (self.entered.get(&edge.to) == Some(&index))
            .then(|| self.rect_of(edge.to))
            .flatten()
    }

    /// The node under `point` (for a repeat, the node it repeats) and where it is drawn there, the topmost first.
    fn hit(&self, point: Pos2) -> Option<(NodeId, Rect)> {
        self.hit_link(point).map(|(id, rect, _)| (id, rect))
    }

    /// [`Self::hit`] leaving out `except` (a node being dragged and dropped).
    fn hit_except(&self, point: Pos2, except: NodeId) -> Option<(NodeId, Rect)> {
        self.rects
            .iter()
            .rev()
            .filter(|(id, _)| *id != except)
            .find(|(_, rect)| rect.contains(point))
            .copied()
            .or_else(|| {
                self.repeats
                    .iter()
                    .rev()
                    .filter(|(_, id, _)| *id != except)
                    .find(|(_, _, rect)| rect.contains(point))
                    .map(|(_, id, rect)| (*id, *rect))
            })
    }

    /// [`Self::hit`], with the link the node there is reached by.
    fn hit_link(&self, point: Pos2) -> Option<(NodeId, Rect, Option<usize>)> {
        self.rects
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(point))
            .map(|(id, rect)| (*id, *rect, self.entered.get(id).copied()))
            .or_else(|| {
                self.repeats
                    .iter()
                    .rev()
                    .find(|(_, _, rect)| rect.contains(point))
                    .map(|(link, id, rect)| (*id, *rect, Some(*link)))
            })
    }
}

/// What a click lined up: kept where it was on the screen by scrolling the flowchart (Encore).
#[derive(Clone, Copy, Debug)]
pub(crate) struct FlowAnchor {
    /// The node or repeat clicked.
    item: AnchorItem,
    /// Its top, from the flowchart's top-left, when it was clicked.
    y: f32,
}

/// A node, or a repeat by the link it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnchorItem {
    Node(NodeId),
    Repeat(usize),
}

impl FlowAnchor {
    /// `item` as it is now in `placement`.
    fn at(placement: &Placement, item: AnchorItem, origin: Pos2) -> Option<Self> {
        let rect = Self { item, y: 0.0 }.find(placement)?;
        Some(Self {
            item,
            y: rect.min.y - origin.y,
        })
    }

    /// Where the anchored item is in `placement`.
    fn find(&self, placement: &Placement) -> Option<Rect> {
        match self.item {
            AnchorItem::Node(id) => placement.rect_of(id),
            AnchorItem::Repeat(link) => placement
                .repeats
                .iter()
                .find(|(index, _, _)| *index == link)
                .map(|(_, _, rect)| *rect),
        }
    }
}

/// A link drag in progress.
#[derive(Clone, Copy)]
struct FlowDrag {
    /// Port being dragged from.
    port: Port,
}

/// A node being moved by hand (Encore: drag a node anywhere on the flowchart).
#[derive(Clone, Copy)]
struct NodeDrag {
    /// The node.
    node: NodeId,
    /// Where the node was grabbed, from its top-left.
    grab: Vec2,
    /// The node's top-left when the drag started, from the flowchart's top-left.
    start: Vec2,
}

/// A node being dragged and dropped (Encore's selection tool: a node is not moved but dropped on another node, a
/// button's row or, from the tray, on the flowchart).
#[derive(Clone, Copy, Debug)]
struct NodeDrop {
    /// The node.
    node: NodeId,
    /// Whether it comes from the tray.
    tray: bool,
    /// Where it was grabbed, from its top-left.
    grab: Vec2,
}

/// What dropping a node does.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DropAction {
    /// Change the link of a port.
    Link(Port, Link),
    /// Put a tray item on the flowchart there (its top-left, from the flowchart's).
    Place(NodeId, Vec2),
    /// Nothing.
    Nothing,
}

/// The key of `node` in the project's moved flowchart nodes.
pub(crate) fn node_key(node: NodeId) -> u64 {
    match node {
        NodeId::FirstPlay => FLOWCHART_DISC,
        NodeId::Menu(id) => id.0,
        NodeId::Timeline(id) => id.0,
        NodeId::Slideshow(id) | NodeId::Playlist(id) | NodeId::ChapterPlaylist(id) => id,
    }
}

/// Whether `node` was put on the flowchart from the tray: it has a place there and the disc does not reach it.
pub(crate) fn placed_from_tray(project: &Project, node: NodeId) -> bool {
    if node == NodeId::FirstPlay || !project.flowchart.contains_key(&node_key(node)) {
        return false;
    }
    let (_, edges) = build_graph(project);
    let (reached, _) = tree(&edges, &[]);
    !reached.contains_key(&node)
}

/// Put the nodes that were moved by hand where they were left (`origin` is the flowchart's top-left; the places are
/// kept at zoom 1, so they grow and shrink with the rest of the flowchart).
pub(crate) fn place_moved(
    rects: &mut [(NodeId, Rect)],
    moved: &BTreeMap<u64, FlowPoint>,
    origin: Pos2,
    zoom: f32,
) {
    for (id, rect) in rects.iter_mut() {
        if let Some(point) = moved.get(&node_key(*id)) {
            let min = origin + vec2(point.x as f32, point.y as f32) * zoom;
            *rect = Rect::from_min_size(min, rect.size());
        }
    }
}

/// The top-left of a node dragged by `grab` to `pointer`: never left of or above the flowchart, nor past
/// [`FLOWCHART_LIMIT`].
/// The place to keep for a node left at `offset` from the flowchart's top-left at `zoom`: at zoom 1, within the
/// flowchart's limits.
fn kept_place(offset: Vec2, zoom: f32) -> FlowPoint {
    let limit = FLOWCHART_LIMIT as f32;
    let at = offset / zoom.max(MIN_ZOOM);
    // `as` saturates; the place is within 0..=FLOWCHART_LIMIT.
    FlowPoint {
        x: at.x.clamp(0.0, limit).round() as i32,
        y: at.y.clamp(0.0, limit).round() as i32,
    }
}

fn dragged_min(pointer: Pos2, grab: Vec2, origin: Pos2) -> Pos2 {
    let limit = FLOWCHART_LIMIT as f32;
    let offset = pointer - grab - origin;
    origin + vec2(offset.x.clamp(0.0, limit), offset.y.clamp(0.0, limit))
}

/// Build nodes and edges from the project.
pub(crate) fn build_graph(project: &Project) -> (Vec<Node>, Vec<Edge>) {
    let mut nodes = vec![disc_node(project)];

    for menu in project.menus.values() {
        // A later page is shown in its first page's node.
        if !project.is_menu_page(menu.id) {
            nodes.push(menu_node(project, menu));
        }
    }

    for timeline in project.timelines.values() {
        let title = object_title(&timeline.name, tlf!("Timeline {id}", id = timeline.id.0));
        let mut node = end_action_node(
            NodeId::Timeline(timeline.id),
            title,
            &timeline.end_action,
            project,
        );
        node.media = Some(Media::Chapter(timeline.id, 0));
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
        node.media = slideshow
            .slides
            .first()
            .map(|slide| Media::Asset(slide.asset));
        nodes.push(node);
    }

    for playlist in project.playlists.values() {
        let title = object_title(&playlist.name, tlf!("Playlist {id}", id = playlist.id));
        let mut node = end_action_node(
            NodeId::Playlist(playlist.id),
            title,
            &playlist.end_action,
            project,
        );
        node.media = playlist
            .items
            .first()
            .and_then(|item| item_media(project, *item));
        nodes.push(node);
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
        node.media = item_media(project, PlaylistItem::ChapterPlaylist(playlist.id));
        nodes.push(node);
    }

    let edges = collect_edges(project, &nodes);
    (nodes, edges)
}

/// The picture of a playlist's item (Encore: a playlist shows its first item's): a timeline's first chapter, a
/// slideshow's first slide, a chapter playlist's first entry.
fn item_media(project: &Project, item: PlaylistItem) -> Option<Media> {
    match item {
        PlaylistItem::Timeline(id) => Some(Media::Chapter(id, 0)),
        PlaylistItem::Slideshow(id) => project
            .slideshows
            .get(&id)?
            .slides
            .first()
            .map(|slide| Media::Asset(slide.asset)),
        PlaylistItem::ChapterPlaylist(id) => {
            let playlist = project.chapter_playlists.get(&id)?;
            let chapter = playlist.chapters.first().copied().unwrap_or(0);
            Some(Media::Chapter(playlist.timeline, chapter))
        }
    }
}

/// Encore's marks of a link on the node it leads to: the audio track and subtitle it selects (the disc's first play
/// and buttons can), then the chapter of a timeline it plays from or the button a menu is entered at ("A2 S1 C1",
/// "B2"; a link to the menu's default button shows that button too, as Encore's "B1" for a disc's first play). Empty
/// when it says none of these (a playlist).
pub(crate) fn link_badge(project: &Project, port: &Port, link: &Link) -> String {
    let (audio, subtitle) = match port {
        Port::FirstPlay => (
            project.settings.audio_default,
            project.settings.subtitle_default,
        ),
        Port::Button { menu, index } => project
            .menus
            .get(menu)
            .and_then(|menu| menu.buttons.get(*index))
            .map_or((None, SubtitleDefault::NoChange), |button| {
                (button.audio, button.subtitle)
            }),
        _ => (None, SubtitleDefault::NoChange),
    };
    let mut marks = Vec::new();
    if let Some(track) = audio {
        marks.push(format!("A{track}"));
    }
    if let SubtitleDefault::Stream(track) = subtitle {
        marks.push(format!("S{track}"));
    }
    match link {
        Link::Timeline { chapter, .. } => marks.push(format!("C{}", chapter.saturating_add(1))),
        Link::Menu { id, button } => {
            // A later page and its button: "P1B1" (Encore counts the pages from 0).
            let page = project.menu_page_number(*id);
            let mut mark = if page > 0 {
                format!("P{page}")
            } else {
                String::new()
            };
            if let Some(button) = entry_button(project, *id, *button) {
                mark.push_str(&format!("B{}", button.saturating_add(1)));
            }
            if !mark.is_empty() {
                marks.push(mark);
            }
        }
        _ => {}
    }
    marks.join(" ")
}

/// The button a link to menu `id` enters at: `button`, or for the default the menu's default button when it has it.
fn entry_button(project: &Project, id: MenuId, button: Option<u32>) -> Option<u32> {
    button.or_else(|| {
        let menu = project.menus.get(&id)?;
        let index = usize::try_from(menu.default_button).ok()?;
        (index < menu.buttons.len()).then_some(menu.default_button)
    })
}

/// The trees of the flowchart: for each node the disc reaches, then each node the disc does not reach that was put on
/// the flowchart from the tray (`placed`, Encore: it shows its own links there), its links in the order of its ports
/// (the link's index in `edges` and the node it leads to), `true` for a link to a node already in a tree (nearer its
/// root, or before it in its column), which Encore draws to a faint repeat of its own. Also returns the roots.
pub(crate) fn tree(
    edges: &[Edge],
    placed: &[NodeId],
) -> (BTreeMap<NodeId, Vec<(usize, NodeId, bool)>>, Vec<NodeId>) {
    let mut children = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut roots = Vec::new();
    for root in std::iter::once(NodeId::FirstPlay).chain(placed.iter().copied()) {
        if !seen.insert(root) {
            continue;
        }
        roots.push(root);
        let mut queue = VecDeque::from([root]);
        while let Some(node) = queue.pop_front() {
            let mut list = Vec::new();
            for (index, edge) in edges
                .iter()
                .enumerate()
                .filter(|(_, edge)| edge.source == node)
            {
                let new = seen.insert(edge.to);
                if new {
                    queue.push_back(edge.to);
                }
                list.push((index, edge.to, !new));
            }
            let _ = children.insert(node, list);
        }
    }
    (children, roots)
}

/// Lay the flowchart out as Encore does: the tree from the disc to the right, each node's links in the column right
/// of it in the order of its ports, each node lined up so its link comes in level with the button or end action it
/// leaves from, or below the one before it when there is no room. An item the disc does not reach that was put on
/// the flowchart (it has a place in `moved`) is the root of a tree of its own there; the other items it does not
/// reach go to the tray.
pub(crate) fn layout(
    nodes: &[Node],
    edges: &[Edge],
    origin: Pos2,
    m: &Metrics,
    (moved, focus): (&BTreeMap<u64, FlowPoint>, &BTreeMap<NodeId, Port>),
) -> Placement {
    let mut placed: Vec<NodeId> = nodes
        .iter()
        .map(|node| node.id)
        .filter(|id| *id != NodeId::FirstPlay && moved.contains_key(&node_key(*id)))
        .collect();
    placed.sort_by_key(|id| (kind_order(*id), *id));
    let (children, roots) = tree(edges, &placed);
    let mut placement = Placement::default();
    for root in roots {
        let at = match moved.get(&node_key(root)) {
            Some(point) if root != NodeId::FirstPlay => {
                origin + vec2(point.x as f32, point.y as f32) * m.zoom
            }
            _ => Pos2::new(origin.x + m.margin_left, origin.y + m.margin),
        };
        place_tree(nodes, edges, &children, (root, at), m, &mut placement);
        // The disc's tree keeps its margin; one put on the flowchart stays on it.
        let limit = if root == NodeId::FirstPlay {
            at.y
        } else {
            origin.y
        };
        apply_focus(
            (nodes, edges, &children),
            (root, limit),
            focus,
            m,
            &mut placement,
        );
    }
    let mut tray: Vec<NodeId> = nodes
        .iter()
        .map(|node| node.id)
        .filter(|id| !children.contains_key(id))
        .collect();
    // Within a kind by name, as Encore's tray lists 00001, 00002 … 00011 whatever order they were made in.
    let title = |id: NodeId| find(nodes, id).map_or("", |node| node.title.as_str());
    tray.sort_by(|a, b| (kind_order(*a), title(*a), *a).cmp(&(kind_order(*b), title(*b), *b)));
    placement.tray = tray;
    placement
}

/// A node of the tree being laid out: where its next child may go and how far down it reaches so far.
struct Frame {
    /// The node.
    id: NodeId,
    /// Where it is.
    rect: Rect,
    /// Its next link.
    next: usize,
    /// The highest its next child may start.
    cursor: f32,
    /// The bottom of it and its children so far.
    bottom: f32,
}

/// Lay the tree of `root` out from `at`, each column right of the one before (without recursion, so a long chain of
/// titles cannot run out of stack).
fn place_tree(
    nodes: &[Node],
    edges: &[Edge],
    children: &BTreeMap<NodeId, Vec<(usize, NodeId, bool)>>,
    (root, at): (NodeId, Pos2),
    m: &Metrics,
    out: &mut Placement,
) {
    let Some(first) = find(nodes, root) else {
        return;
    };
    let rect = Rect::from_min_size(at, vec2(node_width(root, m), node_height(first, m)));
    out.rects.push((root, rect));
    let mut stack = vec![Frame {
        id: root,
        rect,
        next: 0,
        cursor: rect.min.y,
        bottom: rect.max.y,
    }];
    while let Some(frame) = stack.last_mut() {
        let link = children
            .get(&frame.id)
            .and_then(|list| list.get(frame.next))
            .copied();
        frame.next = frame.next.saturating_add(1);
        let Some((index, id, repeat)) = link else {
            let done = frame.bottom;
            let _ = stack.pop();
            if let Some(parent) = stack.last_mut() {
                parent.bottom = parent.bottom.max(done);
                parent.cursor = done + m.gap;
            }
            continue;
        };
        let (Some(node), Some(source), Some(edge)) =
            (find(nodes, id), find(nodes, frame.id), edges.get(index))
        else {
            continue;
        };
        // Level with the port it leaves from, unless the node before it is in the way.
        let level = source_port_y(source, frame.rect, &edge.from, m) - entry_offset(node, m);
        let top = level.max(frame.cursor);
        let x = frame.rect.max.x + m.column_gap;
        if repeat {
            let rect =
                Rect::from_min_size(Pos2::new(x, top), vec2(m.node_width, card_height(node, m)));
            out.repeats.push((index, id, rect));
            frame.bottom = frame.bottom.max(rect.max.y);
            frame.cursor = rect.max.y + m.gap;
        } else {
            let rect = Rect::from_min_size(
                Pos2::new(x, top),
                vec2(node_width(id, m), node_height(node, m)),
            );
            out.rects.push((id, rect));
            let _ = out.entered.insert(id, index);
            stack.push(Frame {
                id,
                rect,
                next: 0,
                cursor: top,
                bottom: rect.max.y,
            });
        }
    }
}

/// A piece of a laid-out tree: a node, or a repeat by its place in [`Placement::repeats`].
#[derive(Clone, Copy)]
enum Item {
    Node(NodeId),
    Repeat(usize),
}

/// Line up each node's focused link as Encore does (the link of the button or child last clicked, else the first):
/// its node level with the port it leaves from, the links before it stacked above it and the ones after below,
/// each with all it leads to. Then the tree is moved down if it went above `limit`.
fn apply_focus(
    (nodes, edges, children): (
        &[Node],
        &[Edge],
        &BTreeMap<NodeId, Vec<(usize, NodeId, bool)>>,
    ),
    (root, limit): (NodeId, f32),
    focus: &BTreeMap<NodeId, Port>,
    m: &Metrics,
    out: &mut Placement,
) {
    let rect_at: BTreeMap<NodeId, usize> = out
        .rects
        .iter()
        .enumerate()
        .map(|(at, (id, _))| (*id, at))
        .collect();
    let repeat_at: BTreeMap<usize, usize> = out
        .repeats
        .iter()
        .enumerate()
        .map(|(at, (index, _, _))| (*index, at))
        .collect();
    // The parents of this tree from the root down (each node at its own place, not its repeats).
    let mut order = vec![root];
    let mut next = 0;
    while let Some(parent) = order.get(next).copied() {
        next = next.saturating_add(1);
        for (_, child, repeat) in children.get(&parent).into_iter().flatten() {
            if !repeat && !order.contains(child) {
                order.push(*child);
            }
        }
    }
    let item_of = |&(index, id, repeat): &(usize, NodeId, bool)| {
        if repeat {
            repeat_at.get(&index).map(|at| Item::Repeat(*at))
        } else {
            Some(Item::Node(id))
        }
    };
    // Everything a link leads to: its node, and all its own links lead to.
    let subtree = |item: Item| {
        let mut items = vec![item];
        let mut next = 0;
        while let Some(current) = items.get(next).copied() {
            next = next.saturating_add(1);
            if let Item::Node(id) = current {
                for link in children.get(&id).into_iter().flatten() {
                    if let Some(child) = item_of(link) {
                        items.push(child);
                    }
                }
            }
        }
        items
    };
    let rect_of = |out: &Placement, item: Item| match item {
        Item::Node(id) => rect_at
            .get(&id)
            .and_then(|at| out.rects.get(*at))
            .map(|(_, rect)| *rect),
        Item::Repeat(at) => out.repeats.get(at).map(|(_, _, rect)| *rect),
    };
    let shift = |out: &mut Placement, items: &[Item], by: f32| {
        for item in items {
            let rect = match *item {
                Item::Node(id) => rect_at
                    .get(&id)
                    .and_then(|at| out.rects.get_mut(*at))
                    .map(|(_, rect)| rect),
                Item::Repeat(at) => out.repeats.get_mut(at).map(|(_, _, rect)| rect),
            };
            if let Some(rect) = rect {
                *rect = rect.translate(vec2(0.0, by));
            }
        }
    };
    let extent = |out: &Placement, items: &[Item]| {
        items.iter().filter_map(|item| rect_of(out, *item)).fold(
            None,
            |span: Option<(f32, f32)>, rect| {
                Some(span.map_or((rect.min.y, rect.max.y), |(top, bottom)| {
                    (top.min(rect.min.y), bottom.max(rect.max.y))
                }))
            },
        )
    };
    // From the deepest parents up, so each link's extent includes what was lined up under it.
    for parent in order.iter().rev() {
        let (Some(list), Some(port), Some(source)) = (
            children.get(parent),
            focus.get(parent),
            find(nodes, *parent),
        ) else {
            continue;
        };
        let Some(focused) = list
            .iter()
            .position(|(index, _, _)| edges.get(*index).is_some_and(|edge| edge.from == *port))
        else {
            continue;
        };
        if focused == 0 {
            continue;
        }
        let Some(parent_rect) = rect_at
            .get(parent)
            .and_then(|at| out.rects.get(*at))
            .map(|(_, rect)| *rect)
        else {
            continue;
        };
        // Each link: its items, where its node's top would be level with its port, and its node's top now.
        let mut links = Vec::new();
        for link in list {
            let (Some(item), Some(edge), Some(node)) =
                (item_of(link), edges.get(link.0), find(nodes, link.1))
            else {
                continue;
            };
            let Some(rect) = rect_of(out, item) else {
                continue;
            };
            let level = source_port_y(source, parent_rect, &edge.from, m) - entry_offset(node, m);
            links.push((subtree(item), level, rect.min.y));
        }
        let Some((items, level, top)) = links.get(focused) else {
            continue;
        };
        shift(out, items, level - top);
        let Some(mut above) = extent(out, items).map(|(top, _)| top) else {
            continue;
        };
        let mut below = extent(out, items).map_or(above, |(_, bottom)| bottom);
        for (items, level, top) in links.iter().skip(focused.saturating_add(1)) {
            let Some((first, _)) = extent(out, items) else {
                continue;
            };
            let by = (level - top).max(below + m.gap - first);
            shift(out, items, by);
            below = extent(out, items).map_or(below, |(_, bottom)| bottom);
        }
        for (items, level, top) in links.iter().take(focused).rev() {
            let Some((_, last)) = extent(out, items) else {
                continue;
            };
            let by = (level - top).min(above - m.gap - last);
            shift(out, items, by);
            above = extent(out, items).map_or(above, |(first, _)| first);
        }
    }
    // A tree that went above its limit moves down as a whole.
    let all = subtree(Item::Node(root));
    if let Some((top, _)) = extent(out, &all)
        && top < limit
    {
        shift(out, &all, limit - top);
    }
}

/// The node `id`.
fn find(nodes: &[Node], id: NodeId) -> Option<&Node> {
    nodes.iter().find(|node| node.id == id)
}

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

/// Whether `id` is a title (what an end action belongs to besides a menu).
fn is_title(id: NodeId) -> bool {
    matches!(
        id,
        NodeId::Timeline(_)
            | NodeId::Slideshow(_)
            | NodeId::Playlist(_)
            | NodeId::ChapterPlaylist(_)
    )
}

/// What dropping `drop` at `point` does, as Encore's selection tool does (measured 2026-11-23): on a menu's button
/// row, a title takes that menu at that button as its end action; on another node (or a repeat of one) the dropped
/// item takes that node as its end action, except that a menu dropped on a title becomes the title's end action, and
/// one dropped on the disc becomes the first play; a tray item dropped anywhere else on the flowchart is put there
/// (`origin` is the flowchart's top-left). `grips` are the buttons' rows (and the names' right ends).
pub(crate) fn node_drop(
    placement: &Placement,
    grips: &[(Port, Rect)],
    drop: (NodeId, bool, Vec2),
    point: Pos2,
    origin: Pos2,
) -> DropAction {
    let (dragged, tray, grab) = drop;
    let row = grips.iter().find_map(|(port, grip)| match port {
        Port::Button { menu, index } if grip.contains(point) => Some((*menu, *index)),
        _ => None,
    });
    let action = if let Some((menu, index)) = row {
        match u32::try_from(index) {
            Ok(button) if is_title(dragged) => Some((
                Port::EndAction(dragged),
                Link::Menu {
                    id: menu,
                    button: Some(button),
                },
            )),
            _ => None,
        }
    } else {
        placement
            .hit_except(point, dragged)
            .and_then(|(target, _)| node_onto(dragged, target))
    };
    match action {
        Some((port, link)) => DropAction::Link(port, link),
        None if tray => DropAction::Place(dragged, dragged_min(point, grab, origin) - origin),
        None => DropAction::Nothing,
    }
}

/// The link `dragged` dropped on the node `target` makes (Encore): on the disc, the first play; a menu on a title,
/// the title's end action; otherwise the dragged item's end action.
pub(crate) fn node_onto(dragged: NodeId, target: NodeId) -> Option<(Port, Link)> {
    match target {
        _ if target == dragged => None,
        NodeId::FirstPlay => link_to(dragged).map(|link| (Port::FirstPlay, link)),
        target if matches!(dragged, NodeId::Menu(_)) && is_title(target) => {
            link_to(dragged).map(|link| (Port::EndAction(target), link))
        }
        target if dragged != NodeId::FirstPlay => {
            link_to(target).map(|link| (Port::EndAction(dragged), link))
        }
        _ => None,
    }
}

/// What a link from `port` (a link drag, a pick whip) dropped at `point` links to: a tray item (`tray`, last frame's
/// places), else, within the visible part of the flowchart (`visible`: a node scrolled out of sight lies under the
/// tray or other panels), a menu's button row (that button) or a node or repeat (the item). `None` where `port`
/// cannot link.
pub(crate) fn link_target(
    (placement, grips, tray): (&Placement, &[(Port, Rect)], &[(NodeId, Rect)]),
    visible: Rect,
    port: &Port,
    point: Pos2,
) -> Option<(NodeId, Link)> {
    let target = if let Some((id, _)) = tray.iter().find(|(_, rect)| rect.contains(point)) {
        (*id, link_to(*id)?)
    } else if !visible.contains(point) {
        return None;
    } else {
        let (node, _) = placement.hit(point)?;
        let row = grips.iter().find_map(|(row, grip)| match row {
            Port::Button { menu, index } if grip.contains(point) => Some((*menu, *index)),
            _ => None,
        });
        match row {
            Some((menu, index)) => (
                node,
                Link::Menu {
                    id: menu,
                    button: u32::try_from(index).ok(),
                },
            ),
            None => (node, link_to(node)?),
        }
    };
    port_accepts(port, target.0).then_some(target)
}

/// Build the disc node: its name and its First Play port (Encore's flowchart does not show the Title button; a menu
/// only it opens is in the tray).
fn disc_node(project: &Project) -> Node {
    let first_play = &project.settings.first_play;
    let target = resolve_target(project, first_play);
    Node {
        id: NodeId::FirstPlay,
        title: object_title(&project.settings.name, tl!("Disc").to_owned()),
        ports: vec![(
            Port::FirstPlay,
            port_text(project, tl!("First Play"), first_play, target),
        )],
        media: None,
        pages: Vec::new(),
        picture: true,
    }
}

/// Build a menu node: one port per button, page by page.
fn menu_node(project: &Project, menu: &Menu) -> Node {
    let mut ports = Vec::new();
    let mut pages = Vec::new();
    for page in project.menu_pages(menu.id) {
        let Some(page_menu) = project.menus.get(&page) else {
            continue;
        };
        pages.push(ports.len());
        for (index, button) in page_menu.buttons.iter().enumerate() {
            let label = button_text(index, button);
            let target = resolve_target(project, &button.link);
            ports.push((
                Port::Button { menu: page, index },
                port_text(project, &label, &button.link, target),
            ));
        }
    }
    // Its end action (Encore draws it from the name, as a title's): after the buttons, so the rows keep their places.
    let end = Port::EndAction(NodeId::Menu(menu.id));
    let target = resolve_target(project, &menu.end_action);
    ports.push((
        end,
        port_text(project, tl!("End Action"), &menu.end_action, target),
    ));

    Node {
        id: NodeId::Menu(menu.id),
        title: object_title(
            &crate::menu_editor::menu_title(project, menu.id),
            tlf!("Menu {id}", id = menu.id.0),
        ),
        ports,
        media: Some(Media::Menu(menu.id)),
        pages,
        picture: true,
    }
}

/// A button's row: its text, else its name (a button without text, as one taken from a Blu-ray's menu, M6.430: its
/// name says what it plays), else its number.
fn button_text(index: usize, button: &Button) -> String {
    if !button.label.is_empty() {
        button.label.clone()
    } else if !button.name.is_empty() {
        button.name.clone()
    } else {
        tlf!("Button {n}", n = index.saturating_add(1))
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
        pages: Vec::new(),
        picture: true,
    }
}

/// Collect edges for all ports that resolve to existing nodes.
fn collect_edges(project: &Project, nodes: &[Node]) -> Vec<Edge> {
    let mut edges = Vec::new();

    for node in nodes {
        // The end action (on the name) before the buttons (in their rows below it): the tree's order.
        let ports = node
            .ports
            .iter()
            .filter(|(port, _)| !is_row(port))
            .chain(node.ports.iter().filter(|(port, _)| is_row(port)));
        for (port, _) in ports {
            let Some(link) = source_link(project, port) else {
                continue;
            };
            let Some(to) = resolve_target(project, link) else {
                continue;
            };
            // A button that turns to another page of its own menu: the node shows the pages.
            if to == node.id
                && let (Port::Button { menu, .. }, Link::Menu { id, .. }) = (port, link)
                && id != menu
            {
                continue;
            }
            let chapter = match link {
                Link::Timeline { id, chapter } => Some((*id, *chapter)),
                _ => None,
            };
            // The default too: opening the menu selects its default button (Encore marks it "B1" as well).
            let button = match link {
                Link::Menu { id, button } => {
                    entry_button(project, *id, *button).map(|button| (*id, button))
                }
                _ => None,
            };

            edges.push(Edge {
                from: *port,
                source: node.id,
                to,
                badge: link_badge(project, port, link),
                chapter,
                button,
            });
        }
    }

    edges
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
        Port::ParentalAlternative(id) => project
            .timelines
            .get(id)
            .map(|timeline| &timeline.parental_alternative),
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

/// Resolve a link to an existing graph node (a later page of a menu to its first page's node).
fn resolve_target(project: &Project, link: &Link) -> Option<NodeId> {
    match link {
        Link::Timeline { id, .. } => project
            .timelines
            .contains_key(id)
            .then_some(NodeId::Timeline(*id)),
        Link::Menu { id, .. } => project
            .menus
            .contains_key(id)
            .then(|| NodeId::Menu(project.menu_root(*id))),
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

/// Use an object name when present, otherwise a generated fallback.
fn object_title(name: &str, fallback: String) -> String {
    if name.is_empty() {
        fallback
    } else {
        name.to_owned()
    }
}

/// The order of the kinds in the tray: menus, then titles, then playlists (each kind by name).
fn kind_order(id: NodeId) -> usize {
    match id {
        NodeId::FirstPlay => 0,
        NodeId::Menu(_) => 1,
        NodeId::Timeline(_) | NodeId::Slideshow(_) => 2,
        NodeId::Playlist(_) | NodeId::ChapterPlaylist(_) => 3,
    }
}

/// Width of a node.
fn node_width(id: NodeId, m: &Metrics) -> f32 {
    if id == NodeId::FirstPlay {
        m.disc_width
    } else {
        m.node_width
    }
}

/// Height of a node's picture (0 when pictures are off).
fn picture_height(node: &Node, m: &Metrics) -> f32 {
    match (node.picture, node.id) {
        (false, _) => 0.0,
        (true, NodeId::FirstPlay) => m.disc,
        (true, _) => m.thumb,
    }
}

/// Height of a node's picture and name: all of a repeat or a tray item.
fn card_height(node: &Node, m: &Metrics) -> f32 {
    let name = if node.id == NodeId::FirstPlay {
        m.disc_label
    } else {
        m.name_bar
    };
    picture_height(node, m) + name
}

/// Whether `port` is drawn as a row under a menu's name (a button).
fn is_row(port: &Port) -> bool {
    matches!(port, Port::Button { .. })
}

/// Number of rows under a menu's name: its pages' headings and its buttons.
fn row_count(node: &Node) -> usize {
    if node.pages.is_empty() {
        0
    } else {
        let buttons = node.ports.iter().filter(|(port, _)| is_row(port)).count();
        buttons.saturating_add(node.pages.len())
    }
}

/// Height of a node in the tree.
fn node_height(node: &Node, m: &Metrics) -> f32 {
    match row_count(node) {
        0 => card_height(node, m),
        rows => {
            card_height(node, m) - m.edge
                + m.accent
                + f32::from(u16::try_from(rows).unwrap_or(u16::MAX)) * m.row
        }
    }
}

/// The row of port `index` among a menu's rows (each page's heading takes the row before its first button).
fn port_row(node: &Node, index: usize) -> usize {
    let headings = node.pages.iter().filter(|start| **start <= index).count();
    index.saturating_add(headings)
}

/// Rectangle of row `row` under a menu's name.
fn row_rect(rect: Rect, node: &Node, row: usize, m: &Metrics) -> Rect {
    let row = f32::from(u16::try_from(row).unwrap_or(u16::MAX));
    let top = rect.min.y + card_height(node, m) - m.edge + m.accent + row * m.row;
    Rect::from_min_size(
        Pos2::new(rect.min.x + m.edge, top),
        vec2(rect.width() - m.edge * 2.0, m.row),
    )
}

/// Y of a port: the disc's on its name, an end action's on the name bar, a button's on its row (the buttons come
/// first among a menu's ports).
pub(crate) fn port_y(rect: Rect, node: &Node, index: usize, m: &Metrics) -> f32 {
    match node.ports.get(index) {
        Some((port, _)) if is_row(port) && !node.pages.is_empty() => {
            row_rect(rect, node, port_row(node, index), m).center().y
        }
        _ => rect.min.y + entry_offset(node, m),
    }
}

/// Y of the port `port` of `node` drawn at `rect`: its row, or the middle of its name when its rows are folded.
fn source_port_y(node: &Node, rect: Rect, port: &Port, m: &Metrics) -> f32 {
    match node.ports.iter().position(|(at, _)| at == port) {
        Some(index) => port_y(rect, node, index, m),
        None => rect.min.y + entry_offset(node, m),
    }
}

/// How far down a node its links come in: the middle of its name.
fn entry_offset(node: &Node, m: &Metrics) -> f32 {
    let name = if node.id == NodeId::FirstPlay {
        m.disc_label
    } else {
        m.name_bar
    };
    picture_height(node, m) + name * 0.5
}

/// Size needed by a laid-out flowchart.
fn content_size(placement: &Placement, origin: Pos2) -> Vec2 {
    let far = placement
        .rects
        .iter()
        .map(|(_, rect)| rect.max)
        .chain(placement.repeats.iter().map(|(_, _, rect)| rect.max))
        .fold(origin, |a, b| a.max(b));
    far - origin + vec2(PADDING, PADDING)
}

/// The mark of a node's kind (the Project panel's).
fn node_icon(id: NodeId) -> RowIcon {
    match id {
        NodeId::FirstPlay => RowIcon::Other,
        NodeId::Menu(_) => RowIcon::Menu,
        NodeId::Timeline(_) => RowIcon::Timeline,
        NodeId::Slideshow(_) => RowIcon::Slideshow,
        NodeId::Playlist(_) => RowIcon::Playlist,
        NodeId::ChapterPlaylist(_) => RowIcon::ChapterPlaylist,
    }
}

/// The edge of a node, in its kind's colour (Encore's, measured: a menu's brown, a timeline's blue-grey, a
/// slideshow's violet, a chapter playlist's teal, a playlist's green).
fn border_color(id: NodeId) -> Color32 {
    match id {
        NodeId::FirstPlay => DISC_EDGE.get(),
        NodeId::Menu(_) => Color32::from_rgb(134, 90, 66),
        NodeId::Timeline(_) => Color32::from_rgb(77, 100, 111),
        NodeId::Slideshow(_) => Color32::from_rgb(92, 90, 123),
        NodeId::ChapterPlaylist(_) => Color32::from_rgb(93, 128, 121),
        NodeId::Playlist(_) => Color32::from_rgb(87, 109, 89),
    }
}

/// The line between a node's picture and its name: its edge's colour 32 darker (Encore).
fn divider_color(edge: Color32) -> Color32 {
    let [r, g, b, _] = edge.to_array();
    Color32::from_rgb(
        r.saturating_sub(32),
        g.saturating_sub(32),
        b.saturating_sub(32),
    )
}

/// `color` taken `amount` (0 to 1) of the way to `to`.
fn mix(color: Color32, to: Color32, amount: f32) -> Color32 {
    let t = amount.clamp(0.0, 1.0);
    let [r, g, b, _] = color.to_array();
    let [r2, g2, b2, _] = to.to_array();
    let channel = |a: u8, b: u8| {
        let value = f32::from(a) + (f32::from(b) - f32::from(a)) * t;
        // `as` saturates.
        value.round() as u8
    };
    Color32::from_rgb(channel(r, r2), channel(g, g2), channel(b, b2))
}

/// A rectangle shaded from `top` at its top edge to `bottom` at its bottom edge (Encore's name bars, page headings
/// and the band under a menu's name).
fn shaded_rect(painter: &Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(Shape::mesh(mesh));
}

/// What drawing the flowchart needs.
struct Scene<'a> {
    /// The nodes.
    nodes: &'a [Node],
    /// The links.
    edges: &'a [Edge],
    /// Where the nodes are.
    placement: &'a Placement,
    /// Where the ports are.
    centers: &'a [(Port, Pos2)],
    /// The nodes' own pictures.
    thumbs: &'a BTreeMap<Media, TextureHandle>,
    /// The frames the links' chapters start at, by link.
    frames: &'a BTreeMap<usize, TextureHandle>,
    /// The selected node.
    selected: Option<NodeId>,
    /// The repeat of the selected node that was clicked (by its link), when one was: it glows blue and the
    /// others grey (Encore glows the place clicked blue, every other place of the item grey).
    clicked_repeat: Option<usize>,
    /// The ports whose links are blue (Encore: the selected button's, the selected title's end action).
    lit: Vec<Port>,
    /// The sizes.
    m: Metrics,
}

impl Scene<'_> {
    /// The glow of a place of the selected item: blue where it was clicked (a repeat, `Some(link)`, or the node
    /// itself, `None`), grey elsewhere. `None` for a place of another item.
    fn glow_of(&self, id: NodeId, repeat: Option<usize>) -> Option<Color32> {
        if self.selected != Some(id) {
            return None;
        }
        Some(if repeat == self.clicked_repeat {
            GLOW.get()
        } else {
            GLOW_REPEAT.get()
        })
    }

    /// The picture of `node` reached by link `link`: the frame its chapter starts at, else the node's own; and
    /// whether it is a video's frame (drawn as [`video_picture_rect`] places it).
    fn picture(&self, node: &Node, link: Option<usize>) -> Option<(&TextureHandle, bool)> {
        if let Some(frame) = link.and_then(|index| self.frames.get(&index)) {
            return Some((frame, true));
        }
        let media = node.media?;
        let texture = self.thumbs.get(&media)?;
        Some((texture, matches!(media, Media::Chapter(..))))
    }

    /// The marks of link `link`.
    fn badge(&self, link: Option<usize>) -> &str {
        link.and_then(|index| self.edges.get(index))
            .map_or("", |edge| edge.badge.as_str())
    }
}

/// How a node is drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    /// In the tree: with its rows.
    Tree,
    /// A faint repeat: its picture and name.
    Repeat,
    /// In the tray: its picture and name, without marks of a link.
    Tray,
}

/// Draw the selected item's glow, the links over it (Encore), then the repeats and the nodes of the tree.
fn draw_scene(painter: &Painter, scene: &Scene) {
    if let Some(selected) = scene.selected {
        // Encore glows every place the selected item shows: the one clicked blue, the others grey.
        for (link, id, rect) in &scene.placement.repeats {
            if let Some(color) = scene.glow_of(*id, Some(*link)) {
                draw_glow(painter, *rect, color, &scene.m);
            }
        }
        if let Some(rect) = scene.placement.rect_of(selected) {
            if selected == NodeId::FirstPlay {
                let picture = find(scene.nodes, selected).is_some_and(|node| node.picture);
                draw_disc_glow(painter, rect, picture, &scene.m);
            } else if let Some(color) = scene.glow_of(selected, None) {
                draw_glow(painter, rect, color, &scene.m);
            }
        }
    }
    draw_edges(painter, scene);
    for (link, id, rect) in &scene.placement.repeats {
        if let Some(node) = find(scene.nodes, *id) {
            draw_card(painter, scene, node, *rect, Look::Repeat, Some(*link));
        }
    }
    for (id, rect) in &scene.placement.rects {
        let Some(node) = find(scene.nodes, *id) else {
            continue;
        };
        let link = scene.placement.entered.get(id).copied();
        if *id == NodeId::FirstPlay {
            draw_disc(painter, node, *rect, &scene.m);
        } else {
            draw_card(painter, scene, node, *rect, Look::Tree, link);
            draw_rows(painter, scene, node, *rect);
        }
    }
}

/// The glow around the selected node (Encore): from `color` at its edge out to the ground over [`GLOW_WIDTH`].
fn draw_glow(painter: &Painter, rect: Rect, color: Color32, m: &Metrics) {
    let steps = m.glow.round().clamp(1.0, 64.0);
    // `as` saturates; `steps` is between 1 and 64.
    for step in 1..=(steps as u8) {
        let out = f32::from(step);
        let stroke = Stroke::new(1.0, mix(GROUND.get(), color, (steps + 1.0 - out) / steps));
        let corner = CornerRadius::same(2_u8.saturating_add(step));
        painter.rect_stroke(rect.expand(out - 0.5), corner, stroke, StrokeKind::Middle);
    }
}

/// The selected disc's glow: around its picture's circle and its name's box (Encore).
fn draw_disc_glow(painter: &Painter, rect: Rect, picture: bool, m: &Metrics) {
    let label = Rect::from_min_max(Pos2::new(rect.min.x, rect.max.y - m.disc_label), rect.max);
    draw_glow(painter, label, GLOW.get(), m);
    if !picture {
        return;
    }
    let radius = m.disc * 0.5;
    let center = Pos2::new(rect.center().x, rect.min.y + radius);
    let steps = m.glow.round().clamp(1.0, 64.0);
    // `as` saturates; `steps` is between 1 and 64.
    for step in 1..=(steps as u8) {
        let out = f32::from(step);
        let stroke = Stroke::new(
            1.0,
            mix(GROUND.get(), GLOW.get(), (steps + 1.0 - out) / steps),
        );
        painter.circle_stroke(center, radius + out - 0.5, stroke);
    }
}

/// `edge` with the selected node's glow over it (Encore: a menu's (134, 90, 66) edge shows (149, 108, 92)).
fn lit_edge(edge: Color32, glow: Color32) -> Color32 {
    let [r, g, b, _] = edge.to_array();
    let [gr, gg, gb, _] = glow.to_array();
    let [ground, _, _, _] = GROUND.get().to_array();
    let lift = |channel: u8, glow: u8| {
        let value = f32::from(channel) + (f32::from(glow) - f32::from(ground)) * 0.53;
        // `as` saturates.
        value.round() as u8
    };
    Color32::from_rgb(lift(r, gr), lift(g, gg), lift(b, gb))
}

/// A node's picture (with its kind's mark and its link's marks) over its name, as Encore draws them: the card in
/// its kind's colour, the picture inside an edge of 2, a line 32 darker, the name shaded inside an edge of 1. A
/// repeat's picture and edge are faded halfway into the ground and its name is grey.
fn draw_card(
    painter: &Painter,
    scene: &Scene,
    node: &Node,
    rect: Rect,
    look: Look,
    link: Option<usize>,
) {
    let m = &scene.m;
    let repeat = look == Look::Repeat;
    let mut edge = border_color(node.id);
    if repeat {
        edge = mix(edge, GROUND.get(), REPEAT_FADE);
    }
    // A place of the selected item takes its glow over its edge (Encore).
    let place = if repeat { link } else { None };
    if look == Look::Tray {
        if scene.selected == Some(node.id) {
            edge = lit_edge(edge, GLOW.get());
        }
    } else if let Some(glow) = scene.glow_of(node.id, place) {
        edge = lit_edge(edge, glow);
    }
    let card = Rect::from_min_size(rect.min, vec2(rect.width(), card_height(node, m)));
    painter.rect_filled(card, CornerRadius::same(2), edge);
    let name_top = card.min.y + picture_height(node, m);
    let name = if node.picture {
        let picture = Rect::from_min_max(
            Pos2::new(card.min.x + m.rim, card.min.y + m.rim),
            Pos2::new(card.max.x - m.rim, name_top),
        );
        match scene.picture(node, link) {
            Some((texture, video)) => {
                painter.rect_filled(picture, CornerRadius::ZERO, PICTURE_FILL);
                match video
                    .then(|| video_picture_rect(picture, texture.size_vec2(), m))
                    .flatten()
                {
                    Some(shown) => {
                        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                        painter.image(texture.id(), shown, uv, Color32::WHITE);
                    }
                    None => draw_fitted_tinted(painter, picture, texture, Color32::WHITE),
                }
            }
            None => {
                painter.rect_filled(picture, CornerRadius::ZERO, NO_PICTURE.get());
            }
        }
        if repeat {
            painter.rect_filled(
                picture,
                CornerRadius::ZERO,
                GROUND.get().gamma_multiply(REPEAT_FADE),
            );
        }
        let fade = if repeat { REPEAT_FADE } else { 1.0 };
        // Encore's mark of the kind is 14 x 11, 2 in from the picture's left and 3 from its top, and covers nothing
        // around it (M6.455: the backing was 18 x 16).
        let glyph = Rect::from_min_size(
            picture.min + vec2(2.0, 3.0) * m.zoom,
            vec2(14.0, 11.0) * m.zoom,
        );
        painter.rect_filled(glyph, CornerRadius::same(1), MARK_FILL.gamma_multiply(fade));
        paint_glyph(painter, glyph, node_icon(node.id), fade);
        // A repeat's marks are as clear as a node's (Encore): they say where its link goes in.
        if look != Look::Tray {
            draw_badge(painter, picture, scene.badge(link), m);
        }
        let line = Rect::from_min_max(
            Pos2::new(picture.min.x, name_top),
            Pos2::new(picture.max.x, name_top + m.edge),
        );
        painter.rect_filled(line, CornerRadius::ZERO, divider_color(edge));
        Rect::from_min_max(
            Pos2::new(card.min.x + m.edge, name_top + m.edge),
            card.max - vec2(m.edge, m.edge),
        )
    } else {
        card.shrink(m.edge)
    };
    shaded_rect(painter, name, NAME_TOP.get(), NAME_BOTTOM.get());
    let left = Pos2::new(name.min.x + m.pad, name.center().y);
    let width = name.width() - m.pad * 2.0;
    let color = if repeat {
        TEXT_REPEAT.get()
    } else {
        TEXT_STRONG.get()
    };
    draw_text(painter, left, &node.title, width, color, m.text);
}

/// Where a video's frame of `size` shows in a node's `picture`: as wide as all but the picture's last 2 columns, at its
/// left, as high as its shape makes it but no higher than the picture, centred up and down (Encore: a 16:9 chapter's
/// frame is 80 x 45 at the left of the 82 x 59 picture, black beside and above and below it; a 4:3 one fills the
/// picture's height, stretched a little, M6.454; a menu's picture fills the width). `None` for an empty size.
fn video_picture_rect(picture: Rect, size: Vec2, m: &Metrics) -> Option<Rect> {
    if !(size.x > 0.0 && size.y > 0.0) {
        return None;
    }
    let width = (picture.width() - 2.0 * m.zoom).max(0.0);
    let height = (width * size.y / size.x).min(picture.height());
    Some(Rect::from_min_size(
        Pos2::new(picture.min.x, picture.center().y - height / 2.0),
        vec2(width, height),
    ))
}

/// A link's marks at the top right of a picture: black on half-transparent white, 2 in from the picture's top and
/// right, with 3 before the text and 2 after it and its capitals 2 below its top ("C1" in a backing of 18 by 13,
/// Encore).
fn draw_badge(painter: &Painter, picture: Rect, badge: &str, m: &Metrics) {
    if badge.is_empty() {
        return;
    }
    let galley = painter.layout_no_wrap(badge.to_owned(), chart_font(m.text), BADGE_TEXT);
    let size = vec2(galley.size().x + 5.0 * m.zoom, galley.size().y.floor());
    let inset = 2.0 * m.zoom;
    let at = Pos2::new(picture.max.x - inset - size.x, picture.min.y + inset);
    let backing = Rect::from_min_size(at, size);
    painter.rect_filled(backing, CornerRadius::ZERO, BADGE_FILL);
    painter.galley(backing.min + vec2(3.0, -1.0) * m.zoom, galley, BADGE_TEXT);
}

/// A menu's rows under its name: the band (from the name's lower edge), each page's heading in an edge of its
/// kind's colour, and its buttons (the selected one blue), without an edge around them (Encore).
fn draw_rows(painter: &Painter, scene: &Scene, node: &Node, rect: Rect) {
    let m = &scene.m;
    if node.pages.is_empty() {
        return;
    }
    let mut edge = border_color(node.id);
    if let Some(glow) = scene.glow_of(node.id, None) {
        edge = lit_edge(edge, glow);
    }
    let top = rect.min.y + card_height(node, m) - m.edge;
    let inner = (rect.min.x + m.edge, rect.max.x - m.edge);
    let band = Rect::from_min_max(Pos2::new(inner.0, top), Pos2::new(inner.1, top + m.accent));
    shaded_rect(painter, band, ACCENT_TOP, ACCENT_BOTTOM);
    let body = Rect::from_min_max(
        Pos2::new(inner.0, band.max.y),
        Pos2::new(inner.1, rect.max.y),
    );
    painter.rect_filled(body, CornerRadius::ZERO, ROWS_FILL.get());
    let width = body.width() - m.pad * 2.0;
    for (page, start) in node.pages.iter().enumerate() {
        let row = row_rect(rect, node, start.saturating_add(page), m);
        painter.rect_filled(row.expand2(vec2(m.edge, 0.0)), CornerRadius::ZERO, edge);
        shaded_rect(painter, row, NAME_TOP.get(), NAME_BOTTOM.get());
        let glyph = Rect::from_min_size(
            Pos2::new(row.min.x + m.pad, row.center().y - 6.0 * m.zoom),
            vec2(14.0, 12.0) * m.zoom,
        );
        paint_glyph(painter, glyph, RowIcon::Menu, 1.0);
        let heading = tlf!("Page {n}", n = page);
        let left = Pos2::new(glyph.max.x + m.pad * 2.0, row.center().y);
        draw_text(
            painter,
            left,
            &heading,
            width - 18.0 * m.zoom,
            TEXT_STRONG.get(),
            m.text,
        );
    }
    for (index, (port, text)) in node.ports.iter().enumerate() {
        if !is_row(port) {
            continue;
        }
        let row = row_rect(rect, node, port_row(node, index), m);
        if scene.lit.contains(port) {
            painter.rect_filled(row, CornerRadius::ZERO, ROW_LIT.get());
        }
        let left = Pos2::new(row.min.x + m.pad, row.center().y);
        draw_text(painter, left, text, width, TEXT.get(), m.text);
    }
}

/// The disc: its picture (when pictures are on) over its name in a box.
fn draw_disc(painter: &Painter, node: &Node, rect: Rect, m: &Metrics) {
    if node.picture {
        let radius = m.disc * 0.5;
        let center = Pos2::new(rect.center().x, rect.min.y + radius);
        painter.circle_filled(center, radius, DISC_FILL);
        painter.circle_stroke(center, radius * 0.78, Stroke::new(1.0, DISC_RING));
        painter.circle_stroke(center, radius * 0.56, Stroke::new(1.0, DISC_RING));
        painter.circle_filled(center, radius * 0.32, DISC_HUB);
        painter.circle_filled(center, radius * 0.1, GROUND.get());
    }
    let label = Rect::from_min_max(Pos2::new(rect.min.x, rect.max.y - m.disc_label), rect.max);
    painter.rect_filled(label, CornerRadius::ZERO, DISC_EDGE.get());
    shaded_rect(
        painter,
        label.shrink(m.edge),
        NAME_TOP.get(),
        NAME_BOTTOM.get(),
    );
    let left = Pos2::new(label.min.x + m.pad, label.center().y);
    draw_text(
        painter,
        left,
        &node.title,
        label.width() - m.pad * 2.0,
        TEXT_STRONG.get(),
        m.text,
    );
}

/// One line of text from `left_center`, vertically centred on it, cut off at `max_width` as Encore cuts a name or a
/// button's text 4 short of the edge of its box ("SUMMER LIVE 2", "02 SECOND CO" and the edge of the M after it),
/// without a "…".
fn draw_text(
    painter: &Painter,
    left_center: Pos2,
    text: &str,
    max_width: f32,
    color: Color32,
    size: f32,
) {
    // Too small to read when zoomed far out: left out, as the boxes alone show the tree.
    if size < 3.0 {
        return;
    }
    let galley = painter.layout_no_wrap(text.to_owned(), chart_font(size), color);
    let top = left_center.y - galley.size().y / 2.0;
    // Cut at the pixel, as Encore does (M6.459: it shows the edge of the M of "02 SECOND CO|MMON THEME", M6.456 had
    // read it as leaving the M out); its letters sit 1 further right than egui puts them (the 0 of "01" 5 in from the
    // row's edge, not 4).
    let clip = Rect::from_min_size(
        Pos2::new(left_center.x, top),
        vec2(max_width.max(0.0), galley.size().y),
    );
    let nudge = size / TEXT_SIZE;
    painter
        .with_clip_rect(clip.intersect(painter.clip_rect()))
        .galley(Pos2::new(left_center.x + nudge, top), galley, color);
}

/// Draw the links: each straight from its port into the left of its node, level with its name, with an arrow two
/// thirds along. The selected button's link is blue, over the others.
fn draw_edges(painter: &Painter, scene: &Scene) {
    let m = &scene.m;
    let mut order: Vec<(usize, &Edge)> = scene.edges.iter().enumerate().collect();
    order.sort_by_key(|(_, edge)| scene.lit.contains(&edge.from));
    for (index, edge) in order {
        let Some(from) = center_of(scene.centers, edge.from) else {
            continue;
        };
        let (Some(rect), Some(node)) = (
            scene.placement.target_of(index, edge),
            find(scene.nodes, edge.to),
        ) else {
            continue;
        };
        let to = Pos2::new(rect.min.x, rect.min.y + entry_offset(node, m));
        // One pixel, blue when lit (Encore).
        let (color, width) = if scene.lit.contains(&edge.from) {
            (LINE_LIT, 1.0)
        } else {
            (LINE.get(), 1.0)
        };
        painter.line_segment([from, to], Stroke::new(width, color));
        arrow(painter, from, to, color, m.arrow);
    }
}

/// Where the port `port` is drawn.
fn center_of(centers: &[(Port, Pos2)], port: Port) -> Option<Pos2> {
    centers
        .iter()
        .find(|(at, _)| *at == port)
        .map(|(_, center)| *center)
}

/// An arrow head on the line from `from` to `to`, its tip [`ARROW_AT`] along (Encore's flowchart).
fn arrow(painter: &Painter, from: Pos2, to: Pos2, color: Color32, length: f32) {
    let along = to - from;
    let distance = along.length();
    if distance.is_nan() || distance <= length * 2.0 {
        return;
    }
    let direction = along / distance;
    let tip = from + along * ARROW_AT;
    let back = tip - direction * length;
    let side = vec2(-direction.y, direction.x) * (length * 0.375);
    let notch = tip - direction * (length * 0.9);
    painter.add(Shape::convex_polygon(
        vec![tip, back + side, notch],
        color,
        Stroke::NONE,
    ));
    painter.add(Shape::convex_polygon(
        vec![tip, notch, back - side],
        color,
        Stroke::NONE,
    ));
}

/// Where the ports of the nodes in the tree are, and the areas a link is dragged from: a button's row, the right end
/// of a title's name, the disc's name.
fn port_centers(
    nodes: &[Node],
    edges: &[Edge],
    placement: &Placement,
    m: &Metrics,
) -> (Vec<(Port, Pos2)>, Vec<(Port, Rect)>) {
    let mut centers = Vec::new();
    let mut grips = Vec::new();
    for (id, rect) in &placement.rects {
        let Some(node) = find(nodes, *id) else {
            continue;
        };
        for (index, (port, _)) in node.ports.iter().enumerate() {
            let y = port_y(*rect, node, index, m);
            centers.push((*port, Pos2::new(rect.max.x, y)));
            let grip = if is_row(port) && !node.pages.is_empty() {
                row_rect(*rect, node, port_row(node, index), m)
            } else {
                let height = if *id == NodeId::FirstPlay {
                    m.disc_label
                } else {
                    m.name_bar
                };
                Rect::from_min_max(
                    Pos2::new(rect.max.x - END_GRIP, y - height * 0.5),
                    Pos2::new(rect.max.x, y + height * 0.5),
                )
            };
            grips.push((*port, grip));
        }
        // A menu whose buttons are folded ("Show Buttons" off): its links leave from the middle of its name.
        if node.pages.is_empty() && node.ports.is_empty() {
            let middle = Pos2::new(rect.max.x, rect.min.y + entry_offset(node, m));
            for edge in edges.iter().filter(|edge| edge.source == *id) {
                centers.push((edge.from, middle));
            }
        }
    }
    (centers, grips)
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

/// Start a link drag if the pointer pressed a port, a button's row or the right end of a name.
fn begin_flow_drag(
    ui: &mut Ui,
    response: &Response,
    ports: (&[(Port, Pos2)], &[(Port, Rect)]),
    drag: Option<FlowDrag>,
) -> Option<FlowDrag> {
    if !response.drag_started() {
        return drag;
    }

    // A drag starts only after the pointer has moved past egui's threshold, so hit-test where the button was
    // pressed, not where the pointer is now (a quick drag would otherwise miss the port).
    let pointer = ui
        .input(|input| input.pointer.press_origin())
        .or_else(|| response.interact_pointer_pos());
    let state = start_drag(pointer, ports.0, ports.1);
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
    targets: (&Placement, &[(Port, Rect)], &[(NodeId, Rect)]),
) -> Option<FlowDrag> {
    if !response.drag_stopped() {
        return drag;
    }

    if let Some(state) = drag {
        let pointer = response
            .interact_pointer_pos()
            .or_else(|| ui.ctx().pointer_hover_pos());
        if let Some(point) = pointer
            && let Some((_, link)) = link_target(targets, ui.clip_rect(), &state.port, point)
            && let Some((command, base)) = command_for_port(&state.port)
        {
            app.apply_link(command, base, &link);
        }
    }

    let id = flow_drag_id(ui);
    ui.data_mut(|data| {
        data.remove::<FlowDrag>(id);
    });

    None
}

/// Temporary-memory id of the node being moved.
fn node_drag_id() -> Id {
    Id::unique("disccraft_flowchart_node_drag")
}

/// The node being moved, if any.
fn node_drag_state(ui: &Ui) -> Option<NodeDrag> {
    ui.data(|data| data.get_temp::<NodeDrag>(node_drag_id()))
}

/// Forget a node move whose release this flowchart did not see (the pointer is up).
fn clear_stale_node_drag(ui: &Ui) {
    if !ui.input(|input| input.pointer.any_down()) {
        ui.data_mut(|data| data.remove::<NodeDrag>(node_drag_id()));
    }
}

/// Start moving the node under the place the button was pressed (a press on a port drags a link instead).
fn begin_node_drag(ui: &Ui, response: &Response, rects: &[(NodeId, Rect)], origin: Pos2) {
    if !response.drag_started_by(egui::PointerButton::Primary) {
        return;
    }
    let Some(point) = ui
        .input(|input| input.pointer.press_origin())
        .or_else(|| response.interact_pointer_pos())
    else {
        return;
    };
    // The topmost node (drawn last) wins.
    let Some((node, rect)) = rects.iter().rev().find(|(_, rect)| rect.contains(point)) else {
        return;
    };
    let drag = NodeDrag {
        node: *node,
        grab: point - rect.min,
        start: rect.min - origin,
    };
    ui.data_mut(|data| data.insert_temp(node_drag_id(), drag));
}

/// Start dragging the node (or repeat) under the place the button was pressed, to drop it on another node or a
/// button's row (Encore's selection tool; the disc stays).
fn begin_node_drop(ui: &Ui, response: &Response, placement: &Placement) {
    if !response.drag_started_by(egui::PointerButton::Primary) {
        return;
    }
    let Some(point) = ui
        .input(|input| input.pointer.press_origin())
        .or_else(|| response.interact_pointer_pos())
    else {
        return;
    };
    let Some((node, rect)) = placement
        .hit(point)
        .filter(|(node, _)| *node != NodeId::FirstPlay)
    else {
        return;
    };
    egui::DragAndDrop::set_payload(
        ui.ctx(),
        NodeDrop {
            node,
            tray: false,
            grab: point - rect.min,
        },
    );
}

/// Where a dragged node would link, to outline: the button's row under `point`, else the node (or repeat) there.
fn drop_area(
    placement: &Placement,
    grips: &[(Port, Rect)],
    point: Pos2,
    dragged: NodeId,
) -> Option<Rect> {
    grips
        .iter()
        .find(|(port, grip)| is_row(port) && grip.contains(point))
        .map(|(_, grip)| *grip)
        .or_else(|| placement.hit_except(point, dragged).map(|(_, rect)| rect))
}

/// The node being moved follows the pointer, drawn over the others, with its links.
fn follow_pointer(ui: &Ui, placement: &mut Placement, drag: Option<NodeDrag>, origin: Pos2) {
    let Some(drag) = drag else {
        return;
    };
    let Some(pointer) = ui.ctx().pointer_interact_pos() else {
        return;
    };
    let Some(index) = placement.rects.iter().position(|(id, _)| *id == drag.node) else {
        return;
    };
    let (id, rect) = placement.rects.remove(index);
    let min = dragged_min(pointer, drag.grab, origin);
    placement
        .rects
        .push((id, Rect::from_min_size(min, rect.size())));
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
}

/// Leave a moved node where it was dropped (one undoable step).
fn finish_node_drag(
    app: &mut DiscCraftApp,
    ui: &Ui,
    response: &Response,
    placement: &Placement,
    (origin, zoom): (Pos2, f32),
) {
    if !response.drag_stopped() {
        return;
    }
    let Some(drag) = ui.data(|data| data.get_temp::<NodeDrag>(node_drag_id())) else {
        return;
    };
    ui.data_mut(|data| data.remove::<NodeDrag>(node_drag_id()));
    let Some(rect) = placement.rect_of(drag.node) else {
        return;
    };
    let offset = rect.min - origin;
    // Let go where it started (a click that wobbled): nothing to undo.
    if (offset - drag.start).length() < 0.5 {
        return;
    }
    let point = kept_place(offset, zoom);
    let _ = app.run_command(
        "flowchart.moveNode",
        serde_json::json!({ "node": node_key(drag.node), "x": point.x, "y": point.y }),
    );
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

/// The port a press at `pointer` drags a link from: a button's row or the right end of a name, else near a port.
fn start_drag(
    pointer: Option<Pos2>,
    centers: &[(Port, Pos2)],
    grips: &[(Port, Rect)],
) -> Option<FlowDrag> {
    let point = pointer?;
    let port = grips
        .iter()
        .find(|(_, grip)| grip.contains(point))
        .map(|(port, _)| *port)
        .or_else(|| {
            centers
                .iter()
                .find(|(_, center)| distance(point, *center) <= PORT_HIT)
                .map(|(port, _)| *port)
        })?;
    Some(FlowDrag { port })
}

/// Handle a click: on a button's row, select the button; elsewhere on a node, select it (a repeat selects the node
/// it repeats). As in Encore, a click only selects (the Properties panel shows it) and a double click (`open`) also
/// brings its viewer forward.
fn click_node(
    app: &mut DiscCraftApp,
    pointer: Option<Pos2>,
    (placement, grips, edges, origin): (&Placement, &[(Port, Rect)], &[Edge], Pos2),
    open: bool,
) {
    let Some(point) = pointer else {
        return;
    };
    if let Some((port, _)) = grips
        .iter()
        .find(|(port, grip)| matches!(port, Port::Button { .. }) && grip.contains(point))
        && let Port::Button { menu, index } = *port
    {
        pick(app, Selection::Menu(menu), open);
        app.selected_buttons = vec![index];
        // Encore lines the button's link up with its row; the menu stays where it is.
        if let Some(edge) = edges.iter().find(|edge| edge.from == *port) {
            let _ = app.flow_focus.insert(edge.source, *port);
            app.flow_anchor = FlowAnchor::at(placement, AnchorItem::Node(edge.source), origin);
        }
        return;
    }
    let Some((id, rect, link)) = placement.hit_link(point) else {
        return;
    };
    // The place clicked: the node itself or one of its repeats (Encore glows it blue, the item's other places grey).
    let place = if placement.rects.contains(&(id, rect)) {
        AnchorItem::Node(id)
    } else {
        AnchorItem::Repeat(link.unwrap_or_default())
    };
    // Encore lines a clicked node up with the port its link leaves from; the node stays where it is.
    if let Some(edge) = link.and_then(|index| edges.get(index)) {
        let _ = app.flow_focus.insert(edge.source, edge.from);
        app.flow_anchor = FlowAnchor::at(placement, place, origin);
    }
    // Opening a menu reached at a button selects that button (Encore).
    let entry = link
        .and_then(|index| edges.get(index))
        .and_then(|edge| edge.button)
        .filter(|_| open);
    match entry {
        Some((page, button)) => {
            pick(app, Selection::Menu(page), true);
            if let Ok(index) = usize::try_from(button) {
                app.selected_buttons = vec![index];
            }
        }
        None => pick(app, crate::context_menus::node_selection(id), open),
    }
    // After the selection, which forgets the place clicked before.
    app.flow_clicked = Some(place);
}

/// Select `selection` without bringing its viewer forward, and with `open` bring it forward (also when it was
/// selected already, by the first click of a double click).
fn pick(app: &mut DiscCraftApp, selection: Selection, open: bool) {
    if selection == Selection::None {
        return;
    }
    app.select_showing(selection, false);
    if open {
        app.show_viewer(selection);
    }
}

/// Right-click menu of the flowchart: of the node under the pointer when the right button was clicked, or of the
/// background (Encore: すべてのサムネールを表示 / 新規 ▸ / 読み込み ▸ / プレビュー).
fn node_context_menu(app: &mut DiscCraftApp, ui: &Ui, response: &Response, placement: &Placement) {
    let key = Id::unique("flowchart_context_node");
    if response.secondary_clicked() {
        let node = response
            .interact_pointer_pos()
            .and_then(|point| placement.hit(point))
            .map(|(id, _)| id);
        ui.data_mut(|data| data.insert_temp(key, node));
        // Encore と同じく、右クリックでもそのノードを選ぶ（ビューアは前に出さない）。
        if let Some(node) = node {
            let selection = crate::context_menus::node_selection(node);
            if selection != Selection::None {
                app.select_showing(selection, false);
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

/// What hovering a node tells: its whole name, and where its end action goes when no link shows it (stop, the last
/// menu).
fn hover_text(nodes: &[Node], edges: &[Edge], id: NodeId) -> Option<String> {
    let node = find(nodes, id)?;
    let mut lines = vec![node.title.as_str()];
    if node.pages.is_empty() {
        lines.extend(
            node.ports
                .iter()
                .filter(|(port, _)| !edges.iter().any(|edge| edge.from == *port))
                .map(|(_, text)| text.as_str()),
        );
    }
    Some(lines.join("\n"))
}

/// The node (or row) a drop would link: its edge turns gold (Encore: 2 pixels of (199, 138, 17) in place of the
/// node's own).
fn draw_drop_target(painter: &Painter, rect: Rect) {
    painter.rect_stroke(
        rect,
        CornerRadius::same(2),
        Stroke::new(2.0, DROP_EDGE),
        StrokeKind::Inside,
    );
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
        Port::ParentalAlternative(id) => {
            let mut map = Map::new();
            let _ = map.insert("timeline".to_owned(), Value::from(id.0));
            Some(("timeline.setParentalAlternative", map))
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

/// The longest side of a chapter's picture made for the flowchart (twice a node at zoom 1, so it stays sharp when
/// zoomed in).
const FRAME_EDGE: usize = 192;

/// What one frame of the flowchart shows: its nodes, links and sizes, and the nodes' own pictures (made once a
/// frame for the tree and the tray).
struct Chart {
    /// The nodes.
    nodes: Vec<Node>,
    /// The nodes as the tray shows them: always with their pictures (Encore: hiding the thumbnails leaves the
    /// tray's).
    tray_nodes: Vec<Node>,
    /// The links.
    edges: Vec<Edge>,
    /// The sizes.
    m: Metrics,
    /// The nodes' own pictures.
    thumbs: BTreeMap<Media, TextureHandle>,
}

/// Temporary-memory id of where the tray's items were last drawn (a link dragged onto one).
fn tray_rects_id() -> Id {
    Id::unique("disccraft_flowchart_tray_rects")
}

impl DiscCraftApp {
    /// Draw the flowchart (Encore's: the tree, a zoom slider, and the tray of the items the disc never reaches) and
    /// handle clicks and dragging.
    pub(crate) fn flowchart_view(&mut self, ui: &mut Ui) {
        // Encore's flowchart starts under its tab, without a line of help over it (M6.452).
        let mut chart = self.flow_chart();
        chart.thumbs = self.node_thumbnails(ui.ctx(), &chart.tray_nodes);
        let m = Metrics::at(MAX_ZOOM);
        let picture = m.thumb;
        // Room for the tray's scroll bar under its items.
        let tray_height = TRAY_TOP + picture + m.name_bar + TRAY_BOTTOM + 8.0;
        // The Flowchart panel's collapsed link list under the tray.
        let list_height = ui.spacing().interact_size.y + 6.0;
        let full = ui.available_rect_before_wrap();
        let canvas_height = (full.height() - tray_height - list_height - BAR).max(80.0);
        // Encore (M6.453): the flowchart with its vertical bar 3 in from the panel's right and top, and under them a
        // row of the zoom controls, the horizontal bar as far as the vertical one, and a corner.
        let canvas = Rect::from_min_max(
            full.min,
            Pos2::new(
                (full.max.x - BAR - BAR_INSET).max(full.min.x),
                full.min.y + canvas_height,
            ),
        );
        let row = Rect::from_min_max(
            Pos2::new(full.min.x, canvas.max.y),
            Pos2::new(full.max.x, canvas.max.y + BAR),
        );
        ui.painter().rect_filled(
            Rect::from_min_max(full.min, row.max),
            CornerRadius::ZERO,
            GROUND.get(),
        );
        let mut inside = ui.new_child(egui::UiBuilder::new().max_rect(canvas));
        // Encore's flowchart does not fade at an edge with more beyond it.
        inside.spacing_mut().scroll.fade.strength = 0.0;
        // Where the bars scrolled to last frame.
        let scroll_to = Id::unique("flowchart_canvas_scroll_to");
        let mut area = ScrollArea::both()
            .id_salt("flowchart_canvas")
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
        if let Some(to) = ui.data_mut(|data| data.remove_temp::<Vec2>(scroll_to)) {
            area = area.scroll_offset(to);
        }
        let output = area.show(&mut inside, |ui| {
            // M6.525: also while a link is dragged from a port.
            let linking = flow_drag_state(ui).is_some();
            crate::pickwhip::scroll_near_edges_for(ui, linking);
            self.flowchart_canvas(ui, &chart)
        });
        let view = output.inner_rect.size();
        let content = output.content_size;
        let mut offset = output.state.clamped_offset();
        let mut scrolled = false;
        let vertical = Rect::from_min_max(
            Pos2::new(canvas.max.x, canvas.min.y + BAR_INSET),
            Pos2::new(canvas.max.x + BAR, canvas.max.y),
        );
        let span = |offset: f32, view: f32, content: f32| Span {
            offset,
            view,
            content,
        };
        if let Some(y) = scroll_bar(
            ui,
            vertical,
            Axis::Vertical,
            output.id.with("vertical_bar"),
            span(offset.y, view.y, content.y),
        ) {
            offset.y = y;
            scrolled = true;
        }
        let zoom = Rect::from_min_max(
            row.min,
            Pos2::new((row.min.x + ZOOM_WIDTH).min(canvas.max.x), row.max.y),
        );
        let horizontal = Rect::from_min_max(
            Pos2::new(zoom.max.x, row.min.y),
            Pos2::new(canvas.max.x.max(zoom.max.x), row.max.y),
        );
        if let Some(x) = scroll_bar(
            ui,
            horizontal,
            Axis::Horizontal,
            output.id.with("horizontal_bar"),
            span(offset.x, view.x, content.x),
        ) {
            offset.x = x;
            scrolled = true;
        }
        if scrolled {
            ui.data_mut(|data| data.insert_temp(scroll_to, offset));
            ui.ctx().request_repaint();
        }
        // The row's dark lines above and below, across the bar and the corner (Encore).
        for y in [row.min.y + 0.5, row.max.y - 0.5] {
            ui.painter()
                .hline(row.x_range(), y, Stroke::new(1.0, ROW_LINE.get()));
        }
        self.flow_zoom_controls(ui, zoom);
        let _ = ui.advance_cursor_after_rect(Rect::from_min_max(full.min, row.max));
        let tray = output.inner;
        let _ = ScrollArea::horizontal()
            .id_salt("flowchart_tray")
            .auto_shrink([false, true])
            .show(ui, |ui| self.flowchart_tray(ui, &chart, &tray));
    }

    /// The zoom controls at the left of the row under the flowchart (Encore): the small mark zooms out and the large
    /// one in, 10 % a click, between 10 % and 100 %, each dim at its end; the slider between them sets any zoom.
    fn flow_zoom_controls(&mut self, ui: &mut Ui, rect: Rect) {
        let at = |x: f32, y: f32, w: f32, h: f32| {
            Rect::from_min_size(Pos2::new(rect.min.x + x, rect.min.y + y), vec2(w, h))
        };
        // Encore's places in its row of 18: the marks 9 x 5 and 17 x 8, the groove 114 long and 4 high.
        let small = at(14.0, 6.0, 9.0, 5.0);
        let large = at(162.0, 3.0, 17.0, 8.0);
        let groove = at(40.0, 8.0, 114.0, 4.0);
        let step = |zoom: f32, by: f32| {
            (((zoom + by) / ZOOM_STEP).round() * ZOOM_STEP).clamp(MIN_ZOOM, MAX_ZOOM)
        };
        let marks = [
            (
                small,
                -ZOOM_STEP,
                self.flow_zoom <= MIN_ZOOM,
                tl!("Zoom Out"),
                "zoom_out",
            ),
            (
                large,
                ZOOM_STEP,
                self.flow_zoom >= MAX_ZOOM,
                tl!("Zoom In"),
                "zoom_in",
            ),
        ];
        for (mark, by, at_end, tip, key) in marks {
            let color = if at_end {
                ZOOM_MARK_DIM.get()
            } else {
                ZOOM_MARK.get()
            };
            let _ = ui.painter().add(crate::scroll_bars::arrow(
                mark.center(),
                vec2(0.0, -1.0),
                mark.size(),
                color,
            ));
            let hit = Rect::from_center_size(mark.center(), vec2(mark.width() + 8.0, BAR - 2.0));
            if ui
                .interact(hit, Id::unique("flowchart").with(key), Sense::click())
                .on_hover_text(tip)
                .clicked()
            {
                self.flow_zoom = step(self.flow_zoom, by);
            }
        }
        // The groove, its light lower edge, and the thumb along it.
        let painter = ui.painter();
        painter.rect_filled(groove, CornerRadius::ZERO, ROW_LINE.get());
        painter.rect_filled(
            groove.shrink2(vec2(1.0, 1.0)),
            CornerRadius::ZERO,
            GROOVE.get(),
        );
        painter.hline(
            groove.x_range(),
            groove.max.y + 0.5,
            Stroke::new(1.0, GROOVE_LIGHT.get()),
        );
        let travel = groove.width() - ZOOM_THUMB;
        let share = ((self.flow_zoom - MIN_ZOOM) / (MAX_ZOOM - MIN_ZOOM)).clamp(0.0, 1.0);
        let thumb = at(40.0 + share * travel, 3.0, ZOOM_THUMB, BAR - 6.0);
        painter.rect_filled(thumb, CornerRadius::ZERO, ZOOM_THUMB_EDGE.get());
        painter.rect_filled(thumb.shrink(1.0), CornerRadius::ZERO, ZOOM_THUMB_FACE.get());
        let slider = Rect::from_min_max(
            Pos2::new(groove.min.x, rect.min.y + 1.0),
            Pos2::new(groove.max.x, rect.max.y - 1.0),
        );
        let response = ui
            .interact(
                slider,
                Id::unique("flowchart_zoom_slider"),
                Sense::click_and_drag(),
            )
            .on_hover_text(tl!("Zoom"));
        if (response.dragged() || response.is_pointer_button_down_on())
            && let Some(pointer) = response.interact_pointer_pos()
            && travel > 0.0
        {
            let share = ((pointer.x - groove.min.x - ZOOM_THUMB / 2.0) / travel).clamp(0.0, 1.0);
            self.flow_zoom = MIN_ZOOM + share * (MAX_ZOOM - MIN_ZOOM);
        }
    }

    /// The nodes as this flowchart shows them: without pictures when thumbnails are off (Encore: the nodes get
    /// smaller), a menu whose buttons are folded without its rows, the disc named after the project file when it has
    /// no name of its own.
    fn flow_chart(&self) -> Chart {
        let (mut nodes, edges) = build_graph(&self.session.project);
        let mut tray_nodes = nodes.clone();
        let file = self
            .session
            .path
            .as_deref()
            .and_then(std::path::Path::file_stem)
            .map(|stem| stem.to_string_lossy().into_owned());
        for node in &mut nodes {
            if !self.flow_thumbnails {
                node.media = None;
                node.picture = false;
            }
            if let NodeId::Menu(id) = node.id
                && self.flow_hidden_buttons.contains(&id)
            {
                node.ports.clear();
                node.pages.clear();
            }
            if node.id == NodeId::FirstPlay
                && self.session.project.settings.name.is_empty()
                && let Some(file) = &file
            {
                node.title.clone_from(file);
            }
        }
        for node in &mut tray_nodes {
            if let Some(shown) = nodes.iter().find(|shown| shown.id == node.id) {
                node.title.clone_from(&shown.title);
            }
        }
        Chart {
            nodes,
            tray_nodes,
            edges,
            m: Metrics::at(self.flow_zoom),
            thumbs: BTreeMap::new(),
        }
    }

    /// The node selected in the flowchart (a menu's page selects its menu's node), and the selected button.
    fn flow_selection(&self) -> (Option<NodeId>, Vec<Port>) {
        let node = match self.selection {
            Selection::Menu(menu) => NodeId::Menu(self.session.project.menu_root(menu)),
            Selection::Timeline(id) => NodeId::Timeline(id),
            Selection::Slideshow(id) => NodeId::Slideshow(id),
            Selection::Playlist(id) => NodeId::Playlist(id),
            Selection::ChapterPlaylist(id) => NodeId::ChapterPlaylist(id),
            // Nothing selected is the disc, as the Properties panel shows it (Encore opens with the disc selected:
            // it glows and its First Play link is blue).
            Selection::None => return (Some(NodeId::FirstPlay), vec![Port::FirstPlay]),
            Selection::Asset(_) => return (None, Vec::new()),
        };
        // Encore: the selected button's link, else the selected item's end action, is blue (a menu's buttons' links
        // stay as they are when the menu itself is selected).
        let lit = match (self.selection, self.selected_buttons.last()) {
            (Selection::Menu(menu), Some(index)) => Port::Button {
                menu,
                index: *index,
            },
            _ => Port::EndAction(node),
        };
        (Some(node), vec![lit])
    }

    /// Draw the tree and process its interaction; returns the tray's items.
    fn flowchart_canvas(&mut self, ui: &mut Ui, chart: &Chart) -> Vec<NodeId> {
        let (nodes, edges, m) = (&chart.nodes, &chart.edges, &chart.m);
        let moved = &self.session.project.flowchart;
        let focus = &self.flow_focus;
        let mut preview = layout(nodes, edges, Pos2::ZERO, m, (moved, focus));
        place_moved(&mut preview.rects, moved, Pos2::ZERO, m.zoom);
        // At least the visible area, so nodes can be dragged anywhere in it.
        let size = content_size(&preview, Pos2::ZERO).max(ui.available_size());

        let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
        let origin = response.rect.min;
        let mut placement = layout(nodes, edges, origin, m, (moved, focus));
        place_moved(&mut placement.rects, moved, origin, m.zoom);
        // After a click that lined up another link, what was clicked stays where it was on the screen (Encore).
        if let Some(anchor) = self.flow_anchor.take()
            && let Some(rect) = anchor.find(&placement)
        {
            let moved_by = rect.min.y - origin.y - anchor.y;
            if moved_by.abs() > 0.5 {
                ui.scroll_with_delta(vec2(0.0, -moved_by));
            }
        }
        let moving = node_drag_state(ui);
        follow_pointer(ui, &mut placement, moving, origin);
        let (centers, grips) = port_centers(nodes, edges, &placement, m);

        painter.rect_filled(response.rect, CornerRadius::ZERO, GROUND.get());
        let thumbs = &chart.thumbs;
        let frames = self.flow_chapter_frames(ui.ctx(), edges, &placement);
        let (selected, lit) = self.flow_selection();
        // A repeat of the selected item that was clicked: it glows blue, and no link is blue (a repeat shows none of
        // its links), as in Encore.
        let clicked_repeat = match self.flow_clicked {
            Some(AnchorItem::Repeat(index))
                if edges
                    .get(index)
                    .is_some_and(|edge| Some(edge.to) == selected) =>
            {
                Some(index)
            }
            _ => None,
        };
        let lit = if clicked_repeat.is_some() {
            Vec::new()
        } else {
            lit
        };
        let scene = Scene {
            nodes,
            edges,
            placement: &placement,
            centers: &centers,
            thumbs,
            frames: &frames,
            selected,
            clicked_repeat,
            lit,
            m: *m,
        };
        draw_scene(&painter, &scene);

        let mut drag = flow_drag_state(ui);
        // M6.525: Escape takes a link drag back, as it does a pick whip.
        if drag.is_some() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            let id = flow_drag_id(ui);
            ui.data_mut(|data| data.remove::<FlowDrag>(id));
            drag = None;
        }
        drag = begin_flow_drag(ui, &response, (&centers, &grips), drag);
        if drag.is_none() {
            // Encore: the Move tool moves a node; the other tools drag it onto another node or a button's row.
            if self.tool == crate::menu_text::Tool::Move {
                begin_node_drag(ui, &response, &placement.rects, origin);
            } else {
                begin_node_drop(ui, &response, &placement);
            }
        }
        let pointer = ui.ctx().pointer_hover_pos();
        let view = (&painter, &response, &placement, grips.as_slice());
        self.flow_drops(view, drag, pointer, origin);
        let tray_rects: Vec<(NodeId, Rect)> = ui
            .data(|data| data.get_temp(tray_rects_id()))
            .unwrap_or_default();
        let targets = (&placement, grips.as_slice(), tray_rects.as_slice());
        drag = finish_flow_drag(self, ui, &response, drag, targets);
        drag = clear_stale_flow_drag(ui, drag);
        finish_node_drag(self, ui, &response, &placement, (origin, m.zoom));
        clear_stale_node_drag(ui);

        // A link dragged from a port draws the pick whip's line to the pointer (M6.525; Encore draws none here, only
        // turning gold the node a drop would link, which it still does).
        if let Some(state) = drag
            && let Some(point) = pointer
            && let Some(from) = center_of(&centers, state.port)
        {
            let aim = if link_target(targets, ui.clip_rect(), &state.port, point).is_some() {
                crate::pickwhip::Aim::Links
            } else if placement.hit(point).is_some() {
                crate::pickwhip::Aim::Refused
            } else {
                crate::pickwhip::Aim::Nothing
            };
            crate::pickwhip::draw_line(ui.ctx(), from, point, aim);
        }
        if drag.is_none()
            && let Some(point) = pointer
            && let Some((port, _)) = grips.iter().find(|(_, grip)| grip.contains(point))
            && let Some(center) = center_of(&centers, *port)
        {
            // Where a link can be dragged from (Encore shows no ports; DiscCraft marks the one under the pointer).
            painter.circle_filled(center, 3.0, LINE_LIT);
        }

        let open = response.double_clicked();
        if open || response.clicked() {
            let point = response.interact_pointer_pos();
            click_node(self, point, (&placement, &grips, edges, origin), open);
        }
        node_context_menu(self, ui, &response, &placement);
        if drag.is_none()
            && moving.is_none()
            && let Some(text) = response
                .hover_pos()
                .and_then(|point| placement.hit(point))
                .and_then(|(id, _)| hover_text(nodes, edges, id))
        {
            let _ = response.on_hover_text_at_pointer(text);
        }
        placement.tray
    }

    /// Outline where a link drag, a dragged node or a pick whip would link, and apply a node or a pick whip dropped on
    /// the flowchart (Encore: a pick whip dropped on a node, or on a button's row, links to it).
    fn flow_drops(
        &mut self,
        (painter, response, placement, grips): (&Painter, &Response, &Placement, &[(Port, Rect)]),
        drag: Option<FlowDrag>,
        pointer: Option<Pos2>,
        origin: Pos2,
    ) {
        // A node scrolled out of sight lies under the tray or other panels: only the visible part takes drops.
        let visible = painter.clip_rect();
        let point =
            pointer.filter(|point| response.rect.contains(*point) && visible.contains(*point));
        if let Some(state) = drag
            && let Some((id, rect)) = point.and_then(|point| placement.hit(point))
            && port_accepts(&state.port, id)
        {
            draw_drop_target(painter, rect);
        }
        let Some(point) = point else {
            return;
        };
        if let Some(payload) = response.dnd_hover_payload::<NodeDrop>() {
            let drop = (payload.node, payload.tray, payload.grab);
            if let DropAction::Link(..) = node_drop(placement, grips, drop, point, origin)
                && let Some(rect) = drop_area(placement, grips, point, payload.node)
            {
                draw_drop_target(painter, rect);
            }
        }
        // A pick whip over a node or a button's row: outlined when a drop links there, and the tether says whether it
        // does (M6.518).
        if placement.hit(point).is_some() {
            crate::pickwhip::refuse_asset_whip(response);
        }
        if let Some(payload) = response.dnd_hover_payload::<crate::pickwhip::PickWhip>()
            && let Some((_, rect)) = placement.hit(point)
        {
            let links = link_target((placement, grips, &[]), visible, &payload.0, point).is_some();
            crate::pickwhip::aim(&response.ctx, links);
            if links {
                let row = grips.iter().find_map(|(port, grip)| {
                    (matches!(port, Port::Button { .. }) && grip.contains(point)).then_some(*grip)
                });
                draw_drop_target(painter, row.unwrap_or(rect));
            }
        }
        if let Some(payload) = crate::pickwhip::released::<NodeDrop>(response) {
            let drop = (payload.node, payload.tray, payload.grab);
            match node_drop(placement, grips, drop, point, origin) {
                DropAction::Link(port, link) => {
                    if let Some((command, base)) = command_for_port(&port) {
                        self.apply_link(command, base, &link);
                    }
                }
                DropAction::Place(node, at) => {
                    let point = kept_place(at, Metrics::at(self.flow_zoom).zoom);
                    let _ = self.run_command(
                        "flowchart.moveNode",
                        serde_json::json!({ "node": node_key(node), "x": point.x, "y": point.y }),
                    );
                }
                DropAction::Nothing => {}
            }
        }
        // A pick whip on a node links to it, on a menu's button row to that button.
        if let Some(payload) = crate::pickwhip::released::<crate::pickwhip::PickWhip>(response)
            && let Some((_, link)) =
                link_target((placement, grips, &[]), visible, &payload.0, point)
            && let Some((command, base)) = command_for_port(&payload.0)
        {
            self.apply_link(command, base, &link);
        }
    }

    /// The tray (Encore: the items the disc never reaches, each its picture and name, in a row under the zoom
    /// slider). Drag one onto the tree to link it, or onto the flowchart to put it there; click selects it.
    fn flowchart_tray(&mut self, ui: &mut Ui, chart: &Chart, tray: &[NodeId]) {
        // The tray does not zoom (Encore: its items keep their size when the flowchart is zoomed out).
        let full = Metrics::at(MAX_ZOOM);
        let m = &full;
        let picture = m.thumb;
        let full = Rect::from_min_size(
            ui.cursor().min,
            vec2(
                ui.available_width(),
                TRAY_TOP + picture + m.name_bar + TRAY_BOTTOM,
            ),
        );
        ui.painter()
            .rect_filled(full, CornerRadius::ZERO, TRAY.get());
        // An empty tray keeps its place (Encore).
        ui.set_min_height(full.height());
        let frames = BTreeMap::new();
        let placement = Placement::default();
        let scene = Scene {
            nodes: &chart.tray_nodes,
            edges: &chart.edges,
            placement: &placement,
            centers: &[],
            thumbs: &chart.thumbs,
            frames: &frames,
            selected: None,
            clicked_repeat: None,
            lit: Vec::new(),
            m: *m,
        };
        let (selected, _) = self.flow_selection();
        let mut rects = Vec::with_capacity(tray.len());
        ui.add_space(TRAY_TOP);
        ui.horizontal(|ui| {
            // Encore's spacing: 7 in from the left, 14 between items.
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(TRAY_LEFT);
            for id in tray {
                let Some(node) = find(&chart.tray_nodes, *id) else {
                    continue;
                };
                let size = vec2(m.node_width, card_height(node, m));
                let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
                rects.push((*id, rect));
                if selected == Some(*id) {
                    draw_glow(ui.painter(), rect, GLOW.get(), m);
                }
                draw_card(ui.painter(), &scene, node, rect, Look::Tray, None);
                // A node dropped on a tray item links as on a node (Encore: a menu dropped on another takes it as its
                // end action); a pick whip links to it.
                let onto = response
                    .dnd_hover_payload::<NodeDrop>()
                    .and_then(|payload| node_onto(payload.node, *id));
                if onto.is_some() {
                    draw_drop_target(ui.painter(), rect);
                }
                if let Some(payload) = crate::pickwhip::released::<NodeDrop>(&response)
                    && let Some((port, link)) = node_onto(payload.node, *id)
                    && let Some((command, base)) = command_for_port(&port)
                {
                    self.apply_link(command, base, &link);
                }
                let _ = self.pick_whip_target(ui, &response, *id);
                if response.drag_started()
                    && let Some(at) = ui.input(|input| input.pointer.press_origin())
                {
                    response.dnd_set_drag_payload(NodeDrop {
                        node: *id,
                        tray: true,
                        grab: at - rect.min,
                    });
                }
                if response.clicked() || response.double_clicked() {
                    let selection = crate::context_menus::node_selection(*id);
                    pick(self, selection, response.double_clicked());
                }
                let response = response.on_hover_text(node.title.clone());
                let _ = response.context_menu(|ui| self.flow_node_menu(ui, *id));
                ui.add_space(TRAY_GAP);
            }
        });
        // Where the items are, for a link dragged from the flowchart onto one (read there next frame).
        ui.data_mut(|data| data.insert_temp(tray_rects_id(), rects));
        // The node being dragged and dropped follows the pointer.
        if let Some(payload) = egui::DragAndDrop::payload::<NodeDrop>(ui.ctx())
            && let Some(pointer) = ui.ctx().pointer_interact_pos()
            && let Some(node) = find(&chart.nodes, payload.node)
        {
            let layer = egui::LayerId::new(egui::Order::Tooltip, Id::unique("flowchart_node_drop"));
            let painter = ui.ctx().layer_painter(layer);
            let rect = Rect::from_min_size(
                pointer - payload.grab,
                vec2(m.node_width, card_height(node, m)),
            );
            draw_card(&painter, &scene, node, rect, Look::Repeat, None);
        }
    }

    /// The frames the links' chapters start at (Encore: a node reached by a link to a chapter shows that chapter),
    /// by link. One frame not made yet is made a frame, so a flowchart of many chapters shows at once and fills in.
    fn flow_chapter_frames(
        &mut self,
        ctx: &egui::Context,
        edges: &[Edge],
        placement: &Placement,
    ) -> BTreeMap<usize, TextureHandle> {
        let mut frames = BTreeMap::new();
        if !self.flow_thumbnails {
            return frames;
        }
        let mut made = false;
        // The pictures kept are those shown now: a frame of a chapter that moved, or of clips that changed, goes.
        let mut used = BTreeSet::new();
        let mut layouts = BTreeMap::new();
        for (index, edge) in edges.iter().enumerate() {
            let Some((timeline, chapter)) = edge.chapter else {
                continue;
            };
            if placement.target_of(index, edge).is_none() {
                continue;
            }
            let Some((time, poster)) = self.chapter_start(timeline, chapter) else {
                continue;
            };
            let layout = *layouts
                .entry(timeline)
                .or_insert_with(|| self.clips_key(timeline));
            let key = (timeline.0, chapter, time.0, layout);
            let _ = used.insert(key);
            let texture = match self.flow_frames.get(&key) {
                Some(known) => known.clone(),
                None if !made => {
                    made = true;
                    self.chapter_frame(ctx, timeline, (time, poster), key)
                }
                None => {
                    ctx.request_repaint();
                    None
                }
            };
            if let Some(texture) = texture {
                let _ = frames.insert(index, texture);
            }
        }
        self.flow_frames.retain(|key, _| used.contains(key));
        frames
    }

    /// A key of the clips of `timeline` (their files, trims and gaps): a chapter's picture is made again when they
    /// change (a clip moved, trimmed or relinked).
    fn clips_key(&self, timeline: TimelineId) -> u64 {
        let project = &self.session.project;
        let Some(found) = project.timeline(timeline) else {
            return 0;
        };
        let files: Vec<&str> = found
            .videos()
            .iter()
            .map(|asset| {
                project
                    .assets
                    .get(asset)
                    .map_or("", |asset| asset.path.as_str())
            })
            .collect();
        let text = format!("{files:?} {:?} {:?}", found.video_trims, found.video_gaps);
        // FNV-1a.
        text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// Where chapter `chapter` of `timeline` shows its picture: its poster frame, else its start. A first chapter at
    /// the start without a poster takes the one Encore gives a new timeline (`true`: where a video that starts black
    /// first shows something), as Encore's flowchart shows a timeline that fades in.
    fn chapter_start(&self, timeline: TimelineId, chapter: u32) -> Option<(Tick, bool)> {
        let found = self
            .session
            .project
            .timeline(timeline)?
            .chapters
            .get(usize::try_from(chapter).ok()?)?;
        Some(match found.poster {
            Some(poster) => (poster, false),
            None => (found.time, chapter == 0 && found.time <= Tick::ZERO),
        })
    }

    /// The frame of `timeline` at `time` (with `poster`, at a file's start, the poster Encore gives a new timeline),
    /// small, kept under `key` (not kept while its file is still opening).
    fn chapter_frame(
        &mut self,
        ctx: &egui::Context,
        timeline: TimelineId,
        (time, poster): (Tick, bool),
        key: (u64, u32, i64, u64),
    ) -> Option<TextureHandle> {
        let made = self.chapter_frame_value(ctx, timeline, (time, poster), key)?;
        let _ = self.flow_frames.insert(key, made.clone());
        made
    }

    /// The chapter picture [`Self::chapter_frame`] makes: `None` while the file is still opening (asked again
    /// later), `Some(None)` when there is no picture there.
    fn chapter_frame_value(
        &mut self,
        ctx: &egui::Context,
        timeline: TimelineId,
        (time, poster): (Tick, bool),
        key: (u64, u32, i64, u64),
    ) -> Option<Option<TextureHandle>> {
        let spans = self.timeline_clips(timeline);
        let Some((clip, seconds)) = crate::playback::clip_at(&spans, time.seconds()) else {
            return Some(None);
        };
        // Not waited for: asked again once the file is open.
        let value = self.run_query_quiet(
            "asset.frame",
            serde_json::json!({
                "asset": clip.asset.0,
                "seconds": seconds.max(0.0),
                "poster": poster,
                "bars": false,
                "wait": false,
            }),
        );
        if crate::project_panel::is_pending(value.as_ref()) {
            self.media_pending = true;
            return None;
        }
        let name = format!("flow_frame_{}_{}_{}", key.0, key.1, key.2);
        Some(
            value
                .as_ref()
                .and_then(|value| small_texture(ctx, &name, value)),
        )
    }

    /// Thumbnails of the nodes' media: an asset's (kept per asset by `asset_thumbnail`), a menu's picture (kept per
    /// menu by `menu_thumbnail`). One menu not drawn yet is drawn a frame, so a flowchart of many menus shows at once
    /// and fills in.
    fn node_thumbnails(
        &mut self,
        ctx: &egui::Context,
        nodes: &[Node],
    ) -> BTreeMap<Media, TextureHandle> {
        let mut thumbs = BTreeMap::new();
        let mut drawn = false;
        let mut asked = false;
        let mut used = BTreeSet::new();
        for media in nodes.iter().filter_map(|node| node.media) {
            let texture = match media {
                Media::Asset(asset) => self.asset_thumbnail(ctx, asset),
                Media::Chapter(timeline, chapter) => {
                    self.node_chapter_picture(ctx, (timeline, chapter), &mut asked, &mut used)
                }
                Media::Menu(menu) => match self.kept_menu_thumbnail(menu) {
                    Some(kept) => kept,
                    None if !drawn => {
                        drawn = true;
                        self.menu_thumbnail(ctx, menu)
                    }
                    None => {
                        ctx.request_repaint();
                        None
                    }
                },
            };
            if let Some(texture) = texture {
                let _ = thumbs.insert(media, texture);
            }
        }
        self.flow_node_frames.retain(|key, _| used.contains(key));
        thumbs
    }

    /// The picture of a node that stands for a timeline's chapter (its poster frame), kept like a link's chapter
    /// picture; one not made yet is made a frame (`asked`), and `used` gathers the ones shown.
    fn node_chapter_picture(
        &mut self,
        ctx: &egui::Context,
        (timeline, chapter): (TimelineId, u32),
        asked: &mut bool,
        used: &mut BTreeSet<(u64, u32, i64, u64)>,
    ) -> Option<TextureHandle> {
        let (time, poster) = self.chapter_start(timeline, chapter)?;
        let key = (timeline.0, chapter, time.0, self.clips_key(timeline));
        let _ = used.insert(key);
        if let Some(Some(known)) = self.flow_frames.get(&key) {
            return Some(known.clone());
        }
        if let Some(known) = self.flow_node_frames.get(&key) {
            return known.clone();
        }
        if *asked {
            ctx.request_repaint();
            return None;
        }
        *asked = true;
        match self.chapter_frame_value(ctx, timeline, (time, poster), key) {
            Some(texture) => {
                let _ = self.flow_node_frames.insert(key, texture.clone());
                texture
            }
            None => None,
        }
    }

    /// Show or hide the pictures of the flowchart's boxes. As in Encore, the flowchart is laid out afresh: the links
    /// lined up by clicks are let go.
    pub(crate) fn toggle_flow_thumbnails(&mut self) {
        self.flow_thumbnails = !self.flow_thumbnails;
        self.flow_focus.clear();
        self.flow_anchor = None;
    }

    /// Run a link-changing command with a target. A link to an item put on the flowchart from the tray also gives
    /// the item its place in the tree beside the link (Encore lines up the node of the newest link), in the same
    /// step.
    pub(crate) fn apply_link(&mut self, command: &str, base: Map<String, Value>, link: &Link) {
        let project = &self.session.project;
        let placed = resolve_target(project, link).filter(|node| placed_from_tray(project, *node));
        match with_target(base, link) {
            Ok(params) => {
                let _ = match placed {
                    Some(node) => self.run_command(
                        "flowchart.link",
                        serde_json::json!({
                            "command": command,
                            "params": params,
                            "node": node_key(node),
                        }),
                    ),
                    None => self.run_command(command, params),
                };
            }
            Err(error) => self.status = error,
        }
    }
}

/// The size, in square pixels at most `edge` wide or high, of a picture `asset.frame` gives without its bars: its
/// part of the 720 x 480 frame of a 4:3 screen, whose pixels are 8:9 (M6.451: a 16:9 video's 720 x 360 is 192 x 108;
/// it was shown 2:1).
fn shown_size((width, height): (usize, usize), edge: usize) -> (usize, usize) {
    let width = (width.saturating_mul(8) / 9).max(1);
    let height = height.max(1);
    let longest = width.max(height);
    if longest > edge {
        (
            (width.saturating_mul(edge) / longest).max(1),
            (height.saturating_mul(edge) / longest).max(1),
        )
    } else {
        (width, height)
    }
}

/// `{"width", "height", "rgb"}` (an `asset.frame` without bars) as a texture in its shape (see [`shown_size`]), at
/// most [`FRAME_EDGE`] wide or high.
fn small_texture(ctx: &egui::Context, name: &str, value: &Value) -> Option<TextureHandle> {
    let width = usize::try_from(value.get("width")?.as_u64()?).ok()?;
    let height = usize::try_from(value.get("height")?.as_u64()?).ok()?;
    let rgb: Vec<u8> = value
        .get("rgb")?
        .as_array()?
        .iter()
        .map(|byte| byte.as_u64().and_then(|byte| u8::try_from(byte).ok()))
        .collect::<Option<Vec<u8>>>()?;
    let (to_width, to_height) = shown_size((width, height), FRAME_EDGE);
    let small = if (to_width, to_height) == (width, height) {
        rgb
    } else {
        crate::project_panel::shrink_rgb(&rgb, (width, height), (to_width, to_height))?
    };
    if small.len() != to_width.checked_mul(to_height)?.checked_mul(3)? {
        return None;
    }
    let image = egui::ColorImage::from_rgb([to_width, to_height], &small);
    Some(ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
}

#[cfg(test)]
mod tests {
    //! Tests for the flowchart graph and layout.

    use super::{
        Edge, GLOW, GLOW_REPEAT, MAX_ZOOM, Media, Metrics, NodeId, Placement, Port, Scene,
        build_graph, command_for_port, dragged_min, kept_place, layout, link_badge, link_to,
        node_drop, node_key, place_moved, placed_from_tray, port_accepts, port_y, shown_size, tree,
        video_picture_rect,
    };
    use disccraft_project::{FlowPoint, Link, MenuId, Project, SubtitleDefault, Tick, TimelineId};
    use egui::{Pos2, Rect, vec2};
    use std::collections::BTreeMap;
    use std::path::Path;

    fn edge(source: NodeId, from: Port, to: NodeId) -> Edge {
        Edge {
            from,
            source,
            to,
            badge: String::new(),
            chapter: None,
            button: None,
        }
    }

    /// The tree grows from the disc: each node's links in the order of its ports, every link to a node of its own (a
    /// second link to one node, or a link back, to a repeat).
    #[test]
    fn every_link_leads_to_a_node_of_its_own() {
        let menu = MenuId(2);
        let m = NodeId::Menu(menu);
        let a = NodeId::Timeline(TimelineId(3));
        let b = NodeId::Timeline(TimelineId(4));
        let button = |index| Port::Button { menu, index };
        let edges = vec![
            edge(NodeId::FirstPlay, Port::FirstPlay, m),
            edge(m, button(0), a),
            edge(m, button(1), a),
            edge(m, button(2), b),
            edge(m, button(3), m),
            edge(a, Port::EndAction(a), m),
            edge(b, Port::EndAction(b), a),
        ];
        let (children, roots) = tree(&edges, &[]);
        assert_eq!(roots, [NodeId::FirstPlay]);
        assert_eq!(children.get(&NodeId::FirstPlay), Some(&vec![(0, m, false)]));
        assert_eq!(
            children.get(&m),
            Some(&vec![
                (1, a, false),
                (2, a, true),
                (3, b, false),
                (4, m, true)
            ])
        );
        assert_eq!(children.get(&a), Some(&vec![(5, m, true)]));
        assert_eq!(children.get(&b), Some(&vec![(6, a, true)]));
    }

    /// Laid out as Encore's: the disc on the left, the menu right of it level with the first play, its buttons'
    /// nodes in the next column each level with its row while there is room (then below the one before), the
    /// timeline nothing reaches in the tray, and nothing overlaps.
    #[test]
    fn nodes_line_up_with_the_ports_they_are_linked_from() -> Result<(), String> {
        let mut project = Project::default();
        let asset = project
            .add_asset(Path::new("a.m2v"))
            .map_err(|error| error.to_string())?;
        let mut timelines = Vec::new();
        for _ in 0..2 {
            timelines.push(
                project
                    .add_timeline(asset, None)
                    .map_err(|error| error.to_string())?,
            );
        }
        let (Some(first), Some(lonely)) = (timelines.first(), timelines.get(1)) else {
            return Err("two timelines".to_owned());
        };
        let menu = project.add_menu(None).map_err(|error| error.to_string())?;
        project.menu_mut(menu).ok_or("menu")?.buttons.clear();
        let to_menu = Link::Menu {
            id: menu,
            button: None,
        };
        // Three buttons to the first timeline (chapters 1, 2, 3); it goes back to the menu.
        for chapter in 0..3 {
            let link = Link::Timeline {
                id: *first,
                chapter,
            };
            project
                .add_button(menu, "b".to_owned(), None, link)
                .map_err(|error| error.to_string())?;
        }
        project.timeline_mut(*first).ok_or("timeline")?.end_action = to_menu.clone();
        project.settings.first_play = to_menu;

        let (nodes, edges) = build_graph(&project);
        let m = Metrics::at(1.0);
        let placement = layout(
            &nodes,
            &edges,
            Pos2::ZERO,
            &m,
            (&BTreeMap::new(), &BTreeMap::new()),
        );
        let at = |id: NodeId| {
            placement
                .rects
                .iter()
                .find(|(node, _)| *node == id)
                .map(|(_, rect)| *rect)
                .ok_or(format!("{id:?}"))
        };
        let menu_node = nodes
            .iter()
            .find(|node| node.id == NodeId::Menu(menu))
            .ok_or("menu node")?;
        let disc = at(NodeId::FirstPlay)?;
        let menu_rect = at(NodeId::Menu(menu))?;
        let title = at(NodeId::Timeline(*first))?;
        assert!(disc.max.x < menu_rect.min.x && menu_rect.max.x < title.min.x);
        // The menu comes in level with the disc's name (both names at the same height). Encore's sizes: a picture
        // of 59 in an edge of 2, a name of 17 between a line and an edge of 1, the disc's name 19 high.
        assert!((menu_rect.min.y + 61.0 + 9.5 - (disc.max.y - 9.5)).abs() < 0.01);
        // The first button's node is level with its row: its name's middle at the row's middle.
        let row = port_y(menu_rect, menu_node, 0, &m);
        assert!(
            (title.min.y + 61.0 + 9.5 - row).abs() < 0.01,
            "{title:?} {row}"
        );
        // The rows start on the name's lower edge, after the band (Encore: 79 + 8 below the node's top), the first
        // button under the page's heading.
        let first_row = port_y(menu_rect, menu_node, 0, &m) - 8.5;
        let expected = menu_rect.min.y + 87.0 + 17.0;
        assert!((first_row - expected).abs() < 0.01, "{first_row}");
        // The other two buttons lead to repeats of the timeline, each below the one before, with their chapters.
        let repeats: Vec<_> = placement
            .repeats
            .iter()
            .filter(|(_, id, _)| *id == NodeId::Timeline(*first))
            .collect();
        assert_eq!(repeats.len(), 2);
        let badges: Vec<&str> = repeats
            .iter()
            .filter_map(|(index, _, _)| edges.get(*index).map(|edge| edge.badge.as_str()))
            .collect();
        assert_eq!(badges, ["C2", "C3"]);
        let mut above = title;
        for (_, _, rect) in &repeats {
            assert!(rect.min.y >= above.max.y + 16.9, "{rect:?} {above:?}");
            above = *rect;
        }
        // The timeline goes back to the menu: a repeat of the menu right of it, level with its name.
        assert!(
            placement
                .repeats
                .iter()
                .any(|(_, id, rect)| *id == NodeId::Menu(menu)
                    && rect.min.x > title.max.x
                    && (rect.min.y - title.min.y).abs() < 0.01)
        );
        assert_eq!(placement.tray, vec![NodeId::Timeline(*lonely)]);
        let all: Vec<Rect> = placement
            .rects
            .iter()
            .map(|(_, rect)| *rect)
            .chain(placement.repeats.iter().map(|(_, _, rect)| *rect))
            .collect();
        for (index, rect) in all.iter().enumerate() {
            for other in all.iter().skip(index + 1) {
                assert!(!rect.intersects(*other), "{rect:?} {other:?}");
            }
        }
        Ok(())
    }

    /// A menu's pages are one node: its buttons page by page under "Page 0", "Page 1" headings, a link to a later
    /// page goes to it, and a button that turns the page is not a link of its own.
    #[test]
    fn a_menus_pages_are_one_node() -> Result<(), String> {
        let mut project = Project::default();
        let asset = project
            .add_asset(Path::new("a.m2v"))
            .map_err(|error| error.to_string())?;
        let timeline = project
            .add_timeline(asset, None)
            .map_err(|error| error.to_string())?;
        let menu = project.add_menu(None).map_err(|error| error.to_string())?;
        project.menu_mut(menu).ok_or("menu")?.buttons.clear();
        let page = project
            .add_menu_page(menu)
            .map_err(|error| error.to_string())?;
        let next = Link::Menu {
            id: page,
            button: None,
        };
        project
            .add_button(menu, "Next".to_owned(), None, next)
            .map_err(|error| error.to_string())?;
        let play = Link::Timeline {
            id: timeline,
            chapter: 0,
        };
        project
            .add_button(page, "Play".to_owned(), None, play)
            .map_err(|error| error.to_string())?;
        project.timeline_mut(timeline).ok_or("timeline")?.end_action = Link::Menu {
            id: page,
            button: None,
        };

        let (nodes, edges) = build_graph(&project);
        let menus: Vec<_> = nodes
            .iter()
            .filter(|node| matches!(node.id, NodeId::Menu(_)))
            .collect();
        let [node] = menus.as_slice() else {
            return Err(format!("one menu node: {menus:?}"));
        };
        assert_eq!(node.id, NodeId::Menu(menu));
        assert_eq!(node.pages, vec![0, 1]);
        // Rows: Page 0, Next, Page 1, Play.
        let m = Metrics::at(1.0);
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(84.0, 400.0));
        let (next_y, play_y) = (port_y(rect, node, 0, &m), port_y(rect, node, 1, &m));
        assert!((play_y - next_y - 34.0).abs() < 0.01, "{next_y} {play_y}");
        assert!(
            !edges
                .iter()
                .any(|edge| edge.from == Port::Button { menu, index: 0 })
        );
        assert!(edges.iter().any(|edge| edge.from
            == Port::Button {
                menu: page,
                index: 0
            }
            && edge.source == NodeId::Menu(menu)
            && edge.to == NodeId::Timeline(timeline)));
        assert!(
            edges
                .iter()
                .any(|edge| edge.source == NodeId::Timeline(timeline)
                    && edge.to == NodeId::Menu(menu))
        );
        Ok(())
    }

    /// A menu only the Title button opens is in the tray, as in Encore.
    #[test]
    fn a_menu_only_the_title_button_opens_is_in_the_tray() -> Result<(), String> {
        let mut project = Project::default();
        let menu = project.add_menu(None).map_err(|error| error.to_string())?;
        project.settings.title_remote = Link::Menu {
            id: menu,
            button: None,
        };
        let (nodes, edges) = build_graph(&project);
        let placement = layout(
            &nodes,
            &edges,
            Pos2::ZERO,
            &Metrics::at(1.0),
            (&BTreeMap::new(), &BTreeMap::new()),
        );
        assert_eq!(placement.tray, vec![NodeId::Menu(menu)]);
        Ok(())
    }

    /// Dropping a node does what Encore's selection tool does (measured 2026-11-23): a title dropped on a menu's
    /// button row takes that menu at that button as its end action; a node dropped on another takes it as its end
    /// action, but a menu dropped on a title becomes the title's end action, and on the disc the first play; a tray
    /// item dropped elsewhere is put there; a node of the tree dropped on nothing does nothing.
    #[test]
    fn a_dropped_node_links_as_in_encore() {
        use super::DropAction;
        let menu = NodeId::Menu(MenuId(7));
        let title = NodeId::Timeline(TimelineId(3));
        let other = NodeId::Timeline(TimelineId(4));
        let rect = |x: f32, y: f32| Rect::from_min_size(Pos2::new(x, y), vec2(100.0, 50.0));
        let placement = Placement {
            rects: vec![
                (NodeId::FirstPlay, rect(0.0, 0.0)),
                (title, rect(200.0, 0.0)),
                (menu, rect(400.0, 0.0)),
                (other, rect(200.0, 100.0)),
            ],
            tray: vec![NodeId::Menu(MenuId(9))],
            ..Placement::default()
        };
        let row = Port::Button {
            menu: MenuId(7),
            index: 1,
        };
        let grips = [(row, rect(400.0, 60.0))];
        let at = |x: f32, y: f32| Pos2::new(x, y);
        let drop = |node, tray, point| {
            node_drop(
                &placement,
                &grips,
                (node, tray, vec2(5.0, 5.0)),
                point,
                Pos2::ZERO,
            )
        };
        // A title on a button's row: its end action is that button.
        assert_eq!(
            drop(title, false, at(450.0, 80.0)),
            DropAction::Link(
                Port::EndAction(title),
                Link::Menu {
                    id: MenuId(7),
                    button: Some(1)
                }
            )
        );
        // A title on another title: its end action.
        assert_eq!(
            drop(title, false, at(250.0, 120.0)),
            DropAction::Link(
                Port::EndAction(title),
                Link::Timeline {
                    id: TimelineId(4),
                    chapter: 0
                }
            )
        );
        // A menu on a title: the title's end action.
        let tray_menu = NodeId::Menu(MenuId(9));
        let to_tray_menu = Link::Menu {
            id: MenuId(9),
            button: None,
        };
        assert_eq!(
            drop(tray_menu, true, at(250.0, 20.0)),
            DropAction::Link(Port::EndAction(title), to_tray_menu.clone())
        );
        // A menu on a menu: the dropped menu's end action.
        assert_eq!(
            drop(tray_menu, true, at(450.0, 20.0)),
            DropAction::Link(
                Port::EndAction(tray_menu),
                Link::Menu {
                    id: MenuId(7),
                    button: None
                }
            )
        );
        // On the disc: the first play.
        assert_eq!(
            drop(tray_menu, true, at(50.0, 20.0)),
            DropAction::Link(Port::FirstPlay, to_tray_menu)
        );
        // A tray menu on a button's row, or on nothing: put there; a node of the tree on nothing: nothing.
        assert_eq!(
            drop(tray_menu, true, at(450.0, 80.0)),
            DropAction::Place(tray_menu, vec2(445.0, 75.0))
        );
        assert_eq!(
            drop(tray_menu, true, at(900.0, 900.0)),
            DropAction::Place(tray_menu, vec2(895.0, 895.0))
        );
        assert_eq!(drop(title, false, at(900.0, 900.0)), DropAction::Nothing);
    }

    /// M6.436 (a review): a link dragged (or a pick whip) onto a tray item links to it; onto a menu's button row, to
    /// that button; outside the visible part of the flowchart (a node scrolled out of sight under the tray) to
    /// nothing; and only where its port can link.
    #[test]
    fn a_link_lands_where_it_is_seen() {
        use super::link_target;
        let menu = NodeId::Menu(MenuId(7));
        let title = NodeId::Timeline(TimelineId(3));
        let lonely = NodeId::Timeline(TimelineId(9));
        let rect = |x: f32, y: f32| Rect::from_min_size(Pos2::new(x, y), vec2(100.0, 50.0));
        let placement = Placement {
            rects: vec![
                (menu, rect(0.0, 0.0)),
                (title, rect(200.0, 0.0)),
                (title, rect(200.0, 600.0)),
            ],
            ..Placement::default()
        };
        let row = Port::Button {
            menu: MenuId(7),
            index: 2,
        };
        let grips = [(
            row,
            Rect::from_min_size(Pos2::new(0.0, 30.0), vec2(100.0, 17.0)),
        )];
        let tray = [(lonely, rect(0.0, 500.0))];
        let visible = Rect::from_min_max(Pos2::ZERO, Pos2::new(800.0, 400.0));
        let targets = (&placement, grips.as_slice(), tray.as_slice());
        let end = Port::EndAction(NodeId::Timeline(TimelineId(5)));
        let at = |x: f32, y: f32| Pos2::new(x, y);
        assert_eq!(
            link_target(targets, visible, &end, at(20.0, 520.0)),
            Some((
                lonely,
                Link::Timeline {
                    id: TimelineId(9),
                    chapter: 0
                }
            ))
        );
        assert_eq!(
            link_target(targets, visible, &end, at(20.0, 40.0)).map(|(_, link)| link),
            Some(Link::Menu {
                id: MenuId(7),
                button: Some(2)
            })
        );
        assert_eq!(
            link_target(targets, visible, &end, at(250.0, 20.0)).map(|(node, _)| node),
            Some(title)
        );
        // Under the tray, out of sight: nothing.
        assert_eq!(link_target(targets, visible, &end, at(250.0, 620.0)), None);
        // The Title button takes only a menu.
        assert_eq!(
            link_target(targets, visible, &Port::TitleButton, at(250.0, 20.0)),
            None
        );
    }

    /// A tray item put on the flowchart is shown there with its own links (a tree of its own); the others stay in
    /// the tray. A link to a later page of a menu says the page ("P1B2").
    #[test]
    fn an_item_put_on_the_flowchart_shows_its_links() -> Result<(), String> {
        let mut project = Project::default();
        let asset = project
            .add_asset(Path::new("a.m2v"))
            .map_err(|error| error.to_string())?;
        let timeline = project
            .add_timeline(asset, None)
            .map_err(|error| error.to_string())?;
        let menu = project.add_menu(None).map_err(|error| error.to_string())?;
        project.menu_mut(menu).ok_or("menu")?.buttons.clear();
        let page = project
            .add_menu_page(menu)
            .map_err(|error| error.to_string())?;
        let play = Link::Timeline {
            id: timeline,
            chapter: 0,
        };
        project
            .add_button(menu, "Play".to_owned(), None, play)
            .map_err(|error| error.to_string())?;
        for label in ["One", "Two"] {
            project
                .add_button(page, label.to_owned(), None, Link::NotSet)
                .map_err(|error| error.to_string())?;
        }
        let lonely = project.add_menu(None).map_err(|error| error.to_string())?;
        project.menu_mut(lonely).ok_or("menu")?.buttons.clear();
        let second = Link::Menu {
            id: page,
            button: Some(1),
        };
        project
            .add_button(lonely, "Back".to_owned(), None, second.clone())
            .map_err(|error| error.to_string())?;
        assert_eq!(
            super::link_badge(&project, &Port::FirstPlay, &second),
            "P1B2"
        );
        // Nothing links to `menu` and `lonely`; `menu` is put on the flowchart.
        let (nodes, edges) = build_graph(&project);
        let mut moved = BTreeMap::new();
        moved.insert(menu.0, FlowPoint { x: 300, y: 400 });
        let placement = layout(
            &nodes,
            &edges,
            Pos2::ZERO,
            &Metrics::at(1.0),
            (&moved, &BTreeMap::new()),
        );
        assert_eq!(placement.tray, vec![NodeId::Menu(lonely)]);
        let at = |id: NodeId| {
            placement
                .rects
                .iter()
                .find(|(node, _)| *node == id)
                .map(|(_, rect)| *rect)
        };
        assert_eq!(
            at(NodeId::Menu(menu)).map(|rect| rect.min),
            Some(Pos2::new(300.0, 400.0))
        );
        let title = at(NodeId::Timeline(timeline)).ok_or("its timeline")?;
        assert!(title.min.x > 300.0 + 84.0);
        Ok(())
    }

    /// Encore's marks of a link: the audio and subtitle it selects, then the chapter or the button it goes in at.
    #[test]
    fn a_links_marks_say_where_it_goes_in() -> Result<(), String> {
        let mut project = Project::default();
        let asset = project
            .add_asset(Path::new("a.m2v"))
            .map_err(|error| error.to_string())?;
        let timeline = project
            .add_timeline(asset, None)
            .map_err(|error| error.to_string())?;
        let menu = project.add_menu(None).map_err(|error| error.to_string())?;
        project.settings.audio_default = Some(2);
        project.settings.subtitle_default = SubtitleDefault::Stream(1);
        let start = Link::Timeline {
            id: timeline,
            chapter: 0,
        };
        assert_eq!(link_badge(&project, &Port::FirstPlay, &start), "A2 S1 C1");
        let end = Port::EndAction(NodeId::Timeline(timeline));
        let third = Link::Timeline {
            id: timeline,
            chapter: 2,
        };
        assert_eq!(link_badge(&project, &end, &third), "C3");
        let second_button = Link::Menu {
            id: menu,
            button: Some(1),
        };
        assert_eq!(link_badge(&project, &end, &second_button), "B2");
        // The default button (Encore marks a disc's first play to the menu's default "B1"); none on a menu without
        // buttons.
        let default = Link::Menu {
            id: menu,
            button: None,
        };
        assert_eq!(link_badge(&project, &end, &default), "");
        for name in ["Play", "Scenes"] {
            project
                .add_button(menu, name.to_owned(), None, Link::Stop)
                .map_err(|error| error.to_string())?;
        }
        assert_eq!(link_badge(&project, &end, &default), "B1");
        if let Some(menu) = project.menus.get_mut(&menu) {
            menu.default_button = 1;
        }
        assert_eq!(link_badge(&project, &end, &default), "B2");
        Ok(())
    }

    /// An item put on the flowchart from the tray is one the disc does not reach; once a link reaches it, or
    /// without a place, it is not.
    #[test]
    fn a_placed_item_is_one_the_disc_does_not_reach() -> Result<(), String> {
        let mut project = Project::default();
        let top = project.add_menu(None).map_err(|error| error.to_string())?;
        let other = project.add_menu(None).map_err(|error| error.to_string())?;
        project.settings.first_play = Link::Menu {
            id: top,
            button: None,
        };
        let _ = project
            .add_button(top, "Go".to_owned(), None, Link::Stop)
            .map_err(|error| error.to_string())?;
        assert!(!placed_from_tray(&project, NodeId::Menu(other)));
        let _ = project
            .flowchart
            .insert(other.0, FlowPoint { x: 10, y: 10 });
        assert!(placed_from_tray(&project, NodeId::Menu(other)));
        if let Some(button) = project
            .menus
            .get_mut(&top)
            .and_then(|menu| menu.buttons.first_mut())
        {
            button.link = Link::Menu {
                id: other,
                button: None,
            };
        }
        assert!(!placed_from_tray(&project, NodeId::Menu(other)));
        assert!(!placed_from_tray(&project, NodeId::FirstPlay));
        Ok(())
    }

    /// A link to a menu's default button enters at that button (opening the node selects it), as one to a button
    /// does; a menu without buttons has none to enter at.
    #[test]
    fn a_default_link_enters_at_the_default_button() -> Result<(), String> {
        let mut project = Project::default();
        let menu = project.add_menu(None).map_err(|error| error.to_string())?;
        project.settings.first_play = Link::Menu {
            id: menu,
            button: None,
        };
        let entry = |project: &Project| {
            let (_, edges) = build_graph(project);
            edges
                .iter()
                .find(|edge| edge.from == Port::FirstPlay)
                .and_then(|edge| edge.button)
        };
        assert_eq!(entry(&project), None);
        for name in ["Play", "Scenes"] {
            project
                .add_button(menu, name.to_owned(), None, Link::Stop)
                .map_err(|error| error.to_string())?;
        }
        assert_eq!(entry(&project), Some((menu, 0)));
        if let Some(menu) = project.menus.get_mut(&menu) {
            menu.default_button = 1;
        }
        assert_eq!(entry(&project), Some((menu, 1)));
        Ok(())
    }

    /// Encore's tray lists each kind by name (00001, 00002 … 00011), whatever order the items were made in; a
    /// timeline shows its first chapter's poster, a chapter playlist its first entry's, a playlist its first item's.
    #[test]
    fn the_tray_lists_by_name_and_shows_chapter_posters() -> Result<(), String> {
        let mut project = Project::default();
        let asset = project
            .add_asset(Path::new("a.m2v"))
            .map_err(|error| error.to_string())?;
        let mut made = Vec::new();
        for name in ["00007", "00001", "00011"] {
            made.push(
                project
                    .add_timeline(asset, Some(name.to_owned()))
                    .map_err(|error| error.to_string())?,
            );
        }
        let first = made.first().copied().ok_or("timeline")?;
        let last = made.get(2).copied().ok_or("timeline")?;
        project
            .add_chapter(first, Tick(254_016_000_000), None)
            .map_err(|error| error.to_string())?;
        let chapters = project
            .add_chapter_playlist(first, None)
            .map_err(|error| error.to_string())?;
        project
            .add_chapter_playlist_entry(chapters, 1, None)
            .map_err(|error| error.to_string())?;
        let list = project
            .add_playlist(None)
            .map_err(|error| error.to_string())?;
        project
            .add_playlist_item(list, last)
            .map_err(|error| error.to_string())?;
        project.settings.first_play = Link::NotSet;

        let (nodes, edges) = build_graph(&project);
        let placement = layout(
            &nodes,
            &edges,
            Pos2::ZERO,
            &Metrics::at(1.0),
            (&BTreeMap::new(), &BTreeMap::new()),
        );
        let names: Vec<&str> = placement
            .tray
            .iter()
            .filter_map(|id| nodes.iter().find(|node| node.id == *id))
            .map(|node| node.title.as_str())
            .collect();
        assert_eq!(names.get(..3), Some(&["00001", "00007", "00011"][..]));

        let media = |id: NodeId| {
            nodes
                .iter()
                .find(|node| node.id == id)
                .and_then(|node| node.media)
        };
        assert_eq!(
            media(NodeId::Timeline(first)),
            Some(Media::Chapter(first, 0))
        );
        assert_eq!(
            media(NodeId::ChapterPlaylist(chapters)),
            Some(Media::Chapter(first, 1))
        );
        assert_eq!(media(NodeId::Playlist(list)), Some(Media::Chapter(last, 0)));
        Ok(())
    }

    /// Encore lines up the link of the button last clicked: its node level with the button's row, the links before
    /// it stacked above it and the ones after below; the tree stays below its margin.
    #[test]
    fn the_focused_buttons_link_is_lined_up_with_its_row() -> Result<(), String> {
        let mut project = Project::default();
        let asset = project
            .add_asset(Path::new("a.m2v"))
            .map_err(|error| error.to_string())?;
        let menu = project.add_menu(None).map_err(|error| error.to_string())?;
        let mut titles = Vec::new();
        for _ in 0..4 {
            let timeline = project
                .add_timeline(asset, None)
                .map_err(|error| error.to_string())?;
            let link = Link::Timeline {
                id: timeline,
                chapter: 0,
            };
            project
                .add_button(menu, "b".to_owned(), None, link)
                .map_err(|error| error.to_string())?;
            titles.push(timeline);
        }
        project.settings.first_play = Link::Menu {
            id: menu,
            button: None,
        };
        let (nodes, edges) = build_graph(&project);
        let m = Metrics::at(1.0);
        let third = Port::Button { menu, index: 2 };
        let focus = BTreeMap::from([(NodeId::Menu(menu), third)]);
        let placement = layout(&nodes, &edges, Pos2::ZERO, &m, (&BTreeMap::new(), &focus));
        let at = |id: NodeId| placement.rect_of(id).ok_or(format!("{id:?}"));
        let menu_node = nodes
            .iter()
            .find(|node| node.id == NodeId::Menu(menu))
            .ok_or("menu node")?;
        let menu_rect = at(NodeId::Menu(menu))?;
        let rects: Vec<Rect> = titles
            .iter()
            .map(|id| at(NodeId::Timeline(*id)))
            .collect::<Result<_, _>>()?;
        let [first, second, focused, last] = rects.as_slice() else {
            return Err("four titles".to_owned());
        };
        let row = port_y(menu_rect, menu_node, 2, &m);
        assert!(
            (focused.min.y + 70.5 - row).abs() < 0.01,
            "{focused:?} {row}"
        );
        assert!(second.max.y + 17.0 <= focused.min.y + 0.01);
        assert!(first.max.y + 17.0 <= second.min.y + 0.01);
        assert!(last.min.y >= focused.max.y + 16.99);
        // The titles above went over the top: the whole tree came down to the margin.
        let top = placement
            .rects
            .iter()
            .map(|(_, rect)| rect.min.y)
            .fold(f32::INFINITY, f32::min);
        assert!((top - 16.0).abs() < 0.01, "{top}");
        assert!((first.min.y - 16.0).abs() < 0.01);
        Ok(())
    }

    /// Encore glows the place of the selected item that was clicked blue and every other place of it grey; other
    /// items do not glow.
    #[test]
    fn the_clicked_place_glows_blue_and_the_others_grey() {
        let placement = Placement::default();
        let (thumbs, frames) = (BTreeMap::new(), BTreeMap::new());
        let timeline = NodeId::Timeline(TimelineId(7));
        let mut scene = Scene {
            nodes: &[],
            edges: &[],
            placement: &placement,
            centers: &[],
            thumbs: &thumbs,
            frames: &frames,
            selected: Some(timeline),
            clicked_repeat: None,
            lit: Vec::new(),
            m: Metrics::at(1.0),
        };
        // The node itself was clicked: it is blue, its repeats grey.
        assert_eq!(scene.glow_of(timeline, None), Some(GLOW.get()));
        assert_eq!(scene.glow_of(timeline, Some(4)), Some(GLOW_REPEAT.get()));
        // Its repeat of link 4 was clicked: that one is blue, the node and its other repeats grey.
        scene.clicked_repeat = Some(4);
        assert_eq!(scene.glow_of(timeline, Some(4)), Some(GLOW.get()));
        assert_eq!(scene.glow_of(timeline, None), Some(GLOW_REPEAT.get()));
        assert_eq!(scene.glow_of(timeline, Some(5)), Some(GLOW_REPEAT.get()));
        assert_eq!(scene.glow_of(NodeId::Menu(MenuId(1)), None), None);
    }

    /// The zoom is kept within the slider's range; one that is not a number is 1.
    #[test]
    fn the_zoom_is_kept_in_range() {
        assert_eq!(Metrics::at(f32::NAN), Metrics::at(1.0));
        assert_eq!(Metrics::at(100.0), Metrics::at(MAX_ZOOM));
        // Encore's range: 10 % to 100 %, opening at 100 %.
        assert_eq!(Metrics::at(2.0), Metrics::at(1.0));
        assert!((Metrics::at(0.5).node_width - 43.0).abs() < 0.01);
        assert!((Metrics::at(0.01).node_width - 8.6).abs() < 0.01);
    }

    /// Nodes moved by hand are drawn where they were left; the others keep the automatic layout, and a drag
    /// cannot take a node off the top or left of the flowchart.
    #[test]
    fn moved_nodes_keep_their_place() {
        use disccraft_project::FlowPoint;
        use std::collections::BTreeMap;

        let menu = NodeId::Menu(MenuId(4));
        let timeline = NodeId::Timeline(TimelineId(5));
        let origin = Pos2::new(10.0, 20.0);
        let mut rects = vec![
            (
                NodeId::FirstPlay,
                Rect::from_min_size(origin, vec2(180.0, 60.0)),
            ),
            (
                menu,
                Rect::from_min_size(Pos2::new(270.0, 20.0), vec2(180.0, 80.0)),
            ),
            (
                timeline,
                Rect::from_min_size(Pos2::new(530.0, 20.0), vec2(180.0, 90.0)),
            ),
        ];
        let mut moved = BTreeMap::new();
        moved.insert(node_key(menu), FlowPoint { x: 600, y: 400 });
        moved.insert(node_key(NodeId::FirstPlay), FlowPoint { x: 0, y: 200 });
        place_moved(&mut rects, &moved, origin, 1.0);
        let at = |index: usize| rects.get(index).map(|(_, rect)| *rect);
        assert_eq!(at(0).map(|rect| rect.min), Some(Pos2::new(10.0, 220.0)));
        assert_eq!(
            at(1),
            Some(Rect::from_min_size(
                Pos2::new(610.0, 420.0),
                vec2(180.0, 80.0)
            ))
        );
        assert_eq!(at(2).map(|rect| rect.min), Some(Pos2::new(530.0, 20.0)));

        assert_eq!(node_key(NodeId::FirstPlay), 0);
        assert_eq!(node_key(NodeId::Slideshow(9)), 9);
        let grab = vec2(30.0, 10.0);
        assert_eq!(
            dragged_min(Pos2::new(100.0, 100.0), grab, origin),
            Pos2::new(70.0, 90.0)
        );
        assert_eq!(dragged_min(Pos2::new(0.0, 0.0), grab, origin), origin);
        assert_eq!(
            dragged_min(Pos2::new(1.0e9, 50.0), grab, origin),
            Pos2::new(10.0 + 100_000.0, 40.0)
        );

        // The places are kept at zoom 1: at zoom 0.5 a node is drawn half as far from the top-left, and one left
        // there is kept twice as far (within the limits).
        let mut half = rects.clone();
        place_moved(&mut half, &moved, origin, 0.5);
        let min = half.get(1).map(|(_, rect)| rect.min);
        assert_eq!(min, Some(Pos2::new(310.0, 220.0)));
        assert_eq!(
            kept_place(vec2(300.0, 200.0), 0.5),
            FlowPoint { x: 600, y: 400 }
        );
        assert_eq!(
            kept_place(vec2(80_000.0, -5.0), 0.5),
            FlowPoint { x: 100_000, y: 0 }
        );
    }

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
        // First Play only (Encore's flowchart does not show the Title button).
        assert_eq!(first.ports.len(), 1);

        let menu_node = nodes
            .iter()
            .find(|node| node.id == NodeId::Menu(menu))
            .ok_or_else(|| "missing menu node".to_owned())?;
        // Its two buttons, then its end action (Encore draws a menu's end action from its name).
        assert_eq!(menu_node.ports.len(), 3);
        assert_eq!(
            menu_node.ports.last().map(|(port, _)| *port),
            Some(Port::EndAction(NodeId::Menu(menu)))
        );
        assert_eq!(menu_node.pages, vec![0]);
        // A menu of a plain colour shows its own picture too, as in Encore's flowchart (M6.408: it had no thumbnail,
        // and a menu on a file showed the file, a pop-up's whole PSD with its white Background among them).
        assert_eq!(menu_node.media, Some(Media::Menu(menu)));

        let timeline_node = nodes
            .iter()
            .find(|node| node.id == NodeId::Timeline(timeline))
            .ok_or_else(|| "missing timeline node".to_owned())?;
        assert_eq!(timeline_node.ports.len(), 1);
        assert!(timeline_node.pages.is_empty());

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
                    && edge.to == NodeId::Timeline(timeline)
                    && edge.badge == "C3"
                    && edge.chapter == Some((timeline, 2)))
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
    fn a_frame_without_bars_is_shown_in_its_shape() {
        // A 16:9 video's part of the 4:3 frame, 720 x 360 with pixels of 8:9, is 16:9 (it was shown 2:1).
        assert_eq!(shown_size((720, 360), 192), (192, 108));
        // A 4:3 video fills the frame: 4:3.
        assert_eq!(shown_size((720, 480), 192), (192, 144));
        assert_eq!(shown_size((90, 30), 192), (80, 30));
        // Nothing comes out empty.
        assert_eq!(shown_size((0, 0), 192), (1, 1));
        // A size from the outside is never more than the edge.
        let (width, height) = shown_size((usize::MAX, usize::MAX), 192);
        assert!((1..=192).contains(&width) && (1..=192).contains(&height));
    }

    #[test]
    fn a_video_frame_sits_at_the_left_of_the_picture() {
        let m = Metrics::at(MAX_ZOOM);
        // Encore: an 82 x 59 picture shows a 16:9 chapter at 80 x 45 at its left, 7 from its top.
        let picture = Rect::from_min_size(Pos2::new(10.0, 20.0), vec2(82.0, 59.0));
        let shown = video_picture_rect(picture, vec2(192.0, 108.0), &m);
        assert_eq!(
            shown,
            Some(Rect::from_min_size(Pos2::new(10.0, 27.0), vec2(80.0, 45.0)))
        );
        // A 4:3 one fills the picture's height and keeps the width, stretched a little (Encore: 81 x 59 from a
        // 720 x 480 file, measured in Encore CS6 for M6.454; it was 78.7 wide).
        let shown = video_picture_rect(picture, vec2(192.0, 144.0), &m);
        assert_eq!(
            shown,
            Some(Rect::from_min_size(Pos2::new(10.0, 20.0), vec2(80.0, 59.0)))
        );
        assert_eq!(video_picture_rect(picture, vec2(0.0, 108.0), &m), None);
        assert_eq!(video_picture_rect(picture, vec2(f32::NAN, 108.0), &m), None);
    }
}
