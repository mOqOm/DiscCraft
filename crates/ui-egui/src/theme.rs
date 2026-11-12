//! 画面の見た目（配色、文字の大きさ、余白、パネルの枠、タブ、表の見出し）。
//!
//! 読みやすさの目安にしたのは Encore の画面の明るさの段階（実機で測った値: パネルの地 ≒ #404040、文字 ≒ #D6D6D6、
//! パネルの間に暗い溝、タブは暗い帯の上に置き選択中のタブだけ地と同じ明るさ、表の列見出しは明るい帯）。
//! 色の値と描き方は DiscCraft で決めたもので、Adobe のアイコンや画像などの素材は使わない。

use egui::{
    Color32, Context, CornerRadius, FontFamily, FontId, Frame, Margin, Sense, Stroke, TextStyle,
    Theme, Ui, Visuals,
};

/// パネルの間の溝。
pub(crate) const GUTTER: Color32 = Color32::from_gray(16);
/// パネルの地。
pub(crate) const PANEL: Color32 = Color32::from_gray(60);
/// タブの帯。
pub(crate) const TAB_STRIP: Color32 = Color32::from_gray(43);
/// マウスが乗ったタブ。
const TAB_HOVER: Color32 = Color32::from_gray(52);
/// 表の列見出しの帯。
pub(crate) const HEADER: Color32 = Color32::from_gray(76);
/// 表の 1 行おきの帯。
const STRIPE: Color32 = Color32::from_gray(65);
/// 入力欄やキャンバスの外側。
const FIELD: Color32 = Color32::from_gray(38);
/// ダイアログとポップアップの地。
const WINDOW: Color32 = Color32::from_gray(66);
/// 枠線。
const BORDER: Color32 = Color32::from_gray(28);
/// 文字。
pub(crate) const TEXT: Color32 = Color32::from_gray(216);
/// 見出しや選択中のタブの文字。
pub(crate) const TEXT_STRONG: Color32 = Color32::from_gray(242);
/// 選ばれていないタブなど、控えめな文字。
pub(crate) const TEXT_WEAK: Color32 = Color32::from_gray(156);
/// 選択の地。
const SELECTION: Color32 = Color32::from_rgb(50, 96, 148);
/// 表の選択中の行の地（文字の選択の地より少し暗くして、行の中の選択と見分ける）。
const SELECTION_ROW: Color32 = Color32::from_rgb(44, 78, 116);

/// タブの帯の高さ。
pub(crate) const TAB_HEIGHT: f32 = 24.0;
/// タブの文字の左右の余白。
const TAB_PADDING: f32 = 12.0;
/// パネルの内側の余白（左右と下。上はタブの帯がパネルの縁に付くように 0）。
pub(crate) const GROUP_MARGIN: i8 = 6;
/// 隣のパネルとの間の溝の半分の幅。
const GUTTER_HALF: i8 = 1;

/// 配色と文字の大きさを設定する。OS が明るい表示でも、この暗い配色を使う。
pub fn apply_theme(ctx: &Context) {
    ctx.set_theme(Theme::Dark);
    ctx.style_mut_of(Theme::Dark, |style| {
        style.visuals = visuals();
        style.text_styles = [
            // Encore と同じくらいの大きさ（本文 12 px、行の間隔 19 px ほど）。
            (
                TextStyle::Small,
                FontId::new(10.5, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(12.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(12.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(15.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.0, FontFamily::Monospace),
            ),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(7.0, 3.0);
        style.spacing.button_padding = egui::vec2(7.0, 2.0);
        style.spacing.interact_size.y = 18.0;
    });
}

fn visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = WINDOW;
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.extreme_bg_color = FIELD;
    visuals.faint_bg_color = STRIPE;
    visuals.code_bg_color = FIELD;
    visuals.override_text_color = None;
    visuals.selection.bg_fill = SELECTION;
    visuals.selection.stroke = Stroke::new(1.0, TEXT_STRONG);
    visuals.window_corner_radius = CornerRadius::same(4);
    visuals.menu_corner_radius = CornerRadius::same(3);

    let radius = CornerRadius::same(2);
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = PANEL;
    widgets.noninteractive.weak_bg_fill = PANEL;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.noninteractive.corner_radius = radius;
    for (state, fill, text) in [
        (&mut widgets.inactive, Color32::from_gray(76), TEXT),
        (&mut widgets.hovered, Color32::from_gray(90), TEXT_STRONG),
        (&mut widgets.active, Color32::from_gray(104), TEXT_STRONG),
        (&mut widgets.open, Color32::from_gray(84), TEXT_STRONG),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, BORDER);
        state.fg_stroke = Stroke::new(1.0, text);
        state.corner_radius = radius;
    }
    visuals
}

/// タブのグループ（プロジェクト、モニター、プロパティなど）のパネルの枠。外側の余白が隣との溝になる。
pub(crate) fn group_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .inner_margin(Margin {
            left: GROUP_MARGIN,
            right: GROUP_MARGIN,
            top: 0,
            bottom: GROUP_MARGIN,
        })
        .outer_margin(Margin::same(GUTTER_HALF))
}

/// ツールバーとステータスバーの枠。
pub(crate) fn bar_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .inner_margin(Margin::symmetric(GROUP_MARGIN, 3))
        .outer_margin(Margin::same(GUTTER_HALF))
}

/// 中身を持たず、溝だけを作る枠（右の列のように、パネルを入れ子にする入れ物）。
pub(crate) fn container_frame() -> Frame {
    Frame::new()
}

/// パネルの上端のタブの帯。選択中のタブは地と同じ明るさにして、下の中身とつなげる。
pub(crate) fn tab_bar<T: Copy + PartialEq>(ui: &mut Ui, current: &mut T, tabs: &[(T, String)]) {
    let (row, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), TAB_HEIGHT), Sense::hover());
    // 帯はパネルの内側の余白の分だけ左右に広げ、パネルの幅いっぱいにする。
    let margin = f32::from(GROUP_MARGIN);
    let strip = egui::Rect::from_min_max(
        egui::pos2(row.left() - margin, row.top()),
        egui::pos2(row.right() + margin, row.bottom()),
    );
    let painter = ui.painter().clone();
    painter.rect_filled(strip, 0.0, TAB_STRIP);
    painter.hline(
        strip.x_range(),
        strip.bottom() - 0.5,
        Stroke::new(1.0, BORDER),
    );

    let font = TextStyle::Button.resolve(ui.style());
    let mut x = strip.left();
    for (index, (tab, label)) in tabs.iter().enumerate() {
        let selected = *current == *tab;
        let galley = painter.layout_no_wrap(label.clone(), font.clone(), TEXT);
        let width = galley.size().x + TAB_PADDING * 2.0;
        let rect = egui::Rect::from_min_size(
            egui::pos2(x, strip.top() + 2.0),
            egui::vec2(width, TAB_HEIGHT - 2.0),
        );
        let response = ui.interact(rect, ui.make_persistent_id(("tab", index)), Sense::click());
        let (fill, color) = if selected {
            (Some(PANEL), TEXT_STRONG)
        } else if response.hovered() {
            (Some(TAB_HOVER), TEXT)
        } else {
            (None, TEXT_WEAK)
        };
        if let Some(fill) = fill {
            painter.rect_filled(
                rect,
                CornerRadius {
                    nw: 3,
                    ne: 3,
                    sw: 0,
                    se: 0,
                },
                fill,
            );
        }
        let galley = painter.layout_no_wrap(label.clone(), font.clone(), color);
        let text_pos = egui::pos2(
            rect.left() + TAB_PADDING,
            rect.center().y - galley.size().y / 2.0,
        );
        painter.galley(text_pos, galley, color);
        if response.clicked() {
            *current = *tab;
        }
        x = rect.right() + 1.0;
    }
    ui.add_space(4.0);
}

/// 列見出しのある表。1 行目（見出し）に明るい帯、本体は 1 行おきに薄い帯。
pub(crate) fn table(id_salt: &str) -> egui::Grid {
    table_selecting(id_salt, None)
}

/// [`table`] に、選択中の行（見出しを除いた 0 始まり）を行全体の選択の色で塗る。
pub(crate) fn table_selecting(id_salt: &str, selected: Option<usize>) -> egui::Grid {
    egui::Grid::new(id_salt)
        .min_col_width(48.0)
        .with_row_color(move |row, _style| match row.checked_sub(1) {
            None => Some(HEADER),
            Some(body) if Some(body) == selected => Some(SELECTION_ROW),
            Some(body) => (body % 2 == 1).then_some(STRIPE),
        })
}

/// 表の並べ替え。Encore と同じく、見出しを押すとその列で並び、同じ列をもう一度押すと向きが変わる。向きは
/// 列を変えても保つ（名前 ▼ のまま 種類 を押すと 種類 ▼）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TableSort {
    /// 並べ替えている列（0 始まり）。
    pub(crate) column: usize,
    /// 昇順か。
    pub(crate) ascending: bool,
}

impl Default for TableSort {
    fn default() -> Self {
        Self {
            column: 0,
            ascending: true,
        }
    }
}

/// 並べ替えの鍵。文字は大文字小文字を区別せず、値のない欄は昇順で最後になる。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SortKey {
    Text(String),
    Number(f64),
    None,
}

impl SortKey {
    fn rank(&self) -> u8 {
        match self {
            SortKey::Number(_) => 0,
            SortKey::Text(_) => 1,
            SortKey::None => 2,
        }
    }

    fn compare(&self, other: &SortKey) -> std::cmp::Ordering {
        match (self, other) {
            (SortKey::Number(a), SortKey::Number(b)) => a.total_cmp(b),
            (SortKey::Text(a), SortKey::Text(b)) => a
                .to_lowercase()
                .cmp(&b.to_lowercase())
                .then_with(|| a.cmp(b)),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

/// 表の見出しのセル。並べ替えている列には ▲ / ▼ を添え、押すと並べ替えが変わる。
pub(crate) fn sort_header(
    ui: &mut egui::Ui,
    sort: &mut TableSort,
    column: usize,
    label: &str,
) -> egui::Response {
    let arrow = if sort.column == column {
        if sort.ascending { " ▲" } else { " ▼" }
    } else {
        ""
    };
    let text = egui::RichText::new(format!("{label}{arrow}")).strong();
    let response = ui
        .add(egui::Label::new(text).sense(Sense::click()))
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() {
        if sort.column == column {
            sort.ascending = !sort.ascending;
        } else {
            sort.column = column;
        }
    }
    response
}

/// `rows` を `sort` の列と向きで並べ替える（同じ鍵の行は元の順のまま）。
pub(crate) fn sort_rows<T>(rows: &mut [T], sort: TableSort, key: impl Fn(&T, usize) -> SortKey) {
    rows.sort_by(|a, b| {
        let order = key(a, sort.column).compare(&key(b, sort.column));
        if sort.ascending {
            order
        } else {
            order.reverse()
        }
    });
}

/// 表を Encore と同じく横幅いっぱいに見せる: 見出しの行の最後に空きのセルを置き、見出しの帯と行の帯を
/// パネルの右端まで伸ばす。空きの幅は前のフレームの表の幅から求める。
pub(crate) struct TableFill {
    key: egui::Id,
    /// 空きのセルの幅。
    filler: f32,
    /// 列（空きのセルを除く）と、その後ろの間隔を合わせた幅。
    columns: f32,
}

impl TableFill {
    pub(crate) fn new(ui: &egui::Ui, id_salt: &str) -> Self {
        let key = egui::Id::unique("table_fill").with(id_salt);
        let (total, filler) = ui
            .data(|data| data.get_temp::<(f32, f32)>(key))
            .unwrap_or((0.0, 0.0));
        let columns = (total - filler).max(0.0);
        let filler = (ui.available_width() - columns).max(0.0);
        Self {
            key,
            filler,
            columns,
        }
    }

    /// 見出しの行の最後（`end_row` の前）に置く空きのセル。
    pub(crate) fn cell(&self, ui: &mut egui::Ui) {
        let _ = ui.allocate_space(egui::vec2(self.filler, 0.0));
    }

    /// 行の右端をパネルの右端までにした、行の最後のセル（[`table_row`] に渡す）。
    pub(crate) fn row_end(&self, first: egui::Rect, last: egui::Rect) -> egui::Rect {
        let right = first.min.x + self.columns + self.filler;
        egui::Rect::from_min_max(last.min, egui::pos2(last.max.x.max(right), last.max.y))
    }

    /// 表を描いたあとで、表の幅を覚える（変わったら描き直す）。
    pub(crate) fn store(&self, ui: &egui::Ui, table: egui::Rect) {
        let stored = (table.width(), self.filler);
        let before = ui.data(|data| data.get_temp::<(f32, f32)>(self.key));
        if before.is_none_or(|(width, filler)| {
            (width - stored.0).abs() > 0.5 || (filler - stored.1).abs() > 0.5
        }) {
            ui.data_mut(|data| data.insert_temp(self.key, stored));
            ui.ctx().request_repaint();
        }
    }
}

/// 枠で囲んだグループ（Encore のビルドパネルの「ソース」「保存先」などと同じ）。見出しを押すと中身をたたむ。
/// 枠は横幅いっぱいに広げ、グループの間を少し空ける。
pub(crate) fn group_box(
    ui: &mut egui::Ui,
    id_salt: &str,
    title: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    let _ = egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let _ = egui::CollapsingHeader::new(title)
                .id_salt(id_salt)
                .default_open(true)
                .show(ui, body);
        });
    ui.add_space(4.0);
}

/// 表の 1 行全体を押せる範囲にする。`first` と `last` は行の左端と右端のセル。どの列を押しても、右クリックしても、
/// ピックウィップを落としても、その行になる（Encore のプロジェクトパネルと同じ）。マウスが乗った行は枠で示す。
pub(crate) fn table_row(
    ui: &mut egui::Ui,
    id: egui::Id,
    first: egui::Rect,
    last: egui::Rect,
) -> egui::Response {
    table_row_sensing(ui, id, first, last, egui::Sense::click())
}

/// [`table_row`] whose row can also be dragged (the Project panel's rows drop onto folders).
pub(crate) fn table_row_draggable(
    ui: &mut egui::Ui,
    id: egui::Id,
    first: egui::Rect,
    last: egui::Rect,
) -> egui::Response {
    table_row_sensing(ui, id, first, last, egui::Sense::click_and_drag())
}

fn table_row_sensing(
    ui: &mut egui::Ui,
    id: egui::Id,
    first: egui::Rect,
    last: egui::Rect,
    sense: egui::Sense,
) -> egui::Response {
    let gap = ui.spacing().item_spacing;
    let rect = first
        .union(last)
        .expand2(egui::vec2(gap.x * 0.5, gap.y * 0.5));
    let response = ui.interact(rect, id, sense);
    if response.hovered() {
        let _ = ui.painter().rect_stroke(
            rect,
            0.0,
            ui.visuals().widgets.hovered.bg_stroke,
            egui::StrokeKind::Inside,
        );
    }
    response
}

/// 見出しのない一覧。1 行おきに薄い帯、選択中の行は行全体を選択の色で塗る。
pub(crate) fn list(id_salt: &str, selected: Option<usize>) -> egui::Grid {
    egui::Grid::new(id_salt).with_row_color(move |row, _style| {
        if Some(row) == selected {
            Some(SELECTION_ROW)
        } else {
            (row % 2 == 1).then_some(STRIPE)
        }
    })
}
