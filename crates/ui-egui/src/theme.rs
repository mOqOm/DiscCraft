//! 画面の見た目（配色、文字の大きさ、余白、パネルの枠、タブ、表の見出し）。
//!
//! 読みやすさの目安にしたのは Encore の画面の明るさの段階（実機で測った値: パネルの地 ≒ #404040、文字 ≒ #D6D6D6、
//! パネルの間に暗い溝、タブは暗い帯の上に置き選択中のタブだけ地と同じ明るさ、表の列見出しは明るい帯）。
//! 色の値と描き方は DiscCraft で決めたもので、Adobe のアイコンや画像などの素材は使わない。
//!
//! 明るさ（M6.480）: 暗い表示（Encore の既定の明るさ）と明るい表示を、環境設定 ▸ 表示方法 で切り替える。画面の色は
//! [`Shade`]（暗い表示の色と明るい表示の色の組）で持ち、描くときに今の表示の色になる。明るい表示の地の段階は Encore の
//! 明るさのスライダーをいちばん明るくした画面で測った値（パネル 242、タブの帯 211、パネルの間の溝 102 など）で、文字は
//! Encore（明るい地に 166 の灰色）より濃くして読みやすくした。

use std::sync::atomic::{AtomicBool, Ordering};

use egui::theme::{DefaultStyle, StyleProvider};
use egui::widget_style::{ButtonStyle, StyleArgs, TextEditStyle, WidgetState};
use egui::{
    Color32, Context, CornerRadius, FontFamily, FontId, Frame, Margin, Sense, Stroke, TextStyle,
    Theme, Visuals,
};
use serde::{Deserialize, Serialize};

/// 画面の明るさ（環境設定 ▸ 表示方法。Encore の「明るさ」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Appearance {
    /// 暗い表示（Encore の既定）。
    #[default]
    Dark,
    /// 明るい表示。
    Light,
}

impl Appearance {
    /// 選べる明るさ（環境設定に並べる順）。
    pub(crate) const ALL: [Appearance; 2] = [Appearance::Dark, Appearance::Light];

    /// 表示名。
    pub(crate) fn label(self) -> &'static str {
        match self {
            Appearance::Dark => crate::i18n::tl!("Dark"),
            Appearance::Light => crate::i18n::tl!("Light"),
        }
    }
}

/// 明るい表示か（描くときに [`Shade`] がどちらの色になるか）。
static LIGHT: AtomicBool = AtomicBool::new(false);

/// 今の表示が明るい表示か。
pub(crate) fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

/// 画面の明るさを切り替える（egui の部品の配色と、DiscCraft が描く色の両方。次のフレームから）。
pub(crate) fn set_appearance(ctx: &Context, appearance: Appearance) {
    let light = appearance == Appearance::Light;
    LIGHT.store(light, Ordering::Relaxed);
    ctx.set_theme(if light { Theme::Light } else { Theme::Dark });
    ctx.request_repaint();
}

/// 画面の色: 暗い表示の色と明るい表示の色の組。`Color32` が要るところでは [`Shade::get`] か `into()` で今の表示の
/// 色になる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shade {
    /// 暗い表示の色。
    pub(crate) dark: Color32,
    /// 明るい表示の色。
    pub(crate) light: Color32,
}

impl Shade {
    /// 暗い表示と明るい表示の色。
    pub(crate) const fn new(dark: Color32, light: Color32) -> Self {
        Self { dark, light }
    }

    /// 灰色の組。
    pub(crate) const fn gray(dark: u8, light: u8) -> Self {
        Self::new(Color32::from_gray(dark), Color32::from_gray(light))
    }

    /// どちらの表示でも同じ色（映像の黒や、選んだ印の色など）。
    pub(crate) const fn same(color: Color32) -> Self {
        Self::new(color, color)
    }

    /// `light` の表示の色。
    pub(crate) const fn pick(self, light: bool) -> Color32 {
        if light { self.light } else { self.dark }
    }

    /// 今の表示の色。
    pub(crate) fn get(self) -> Color32 {
        self.pick(is_light())
    }
}

impl From<Shade> for Color32 {
    fn from(shade: Shade) -> Self {
        shade.get()
    }
}

/// 窓の地（メニューバーの裏。暗い表示は今までどおりほぼ黒）。
pub(crate) const APP_BG: Shade = Shade::gray(8, 236);
/// 問題・注意・よい結果の文字（M6.481。色覚の違いでも見分けられる色: 1 型・2 型・3 型色覚のシミュレーション
/// （Machado ほか 2009）で、問題と注意、問題とよい結果の色の差がどれも ΔE 35 以上。暗い表示はばら色・黄・空色、
/// 明るい表示は濃い紅・濃い黄土・青）。色だけに頼らず、[`status_label`] が形の違う印も付ける。
pub(crate) const ERROR_TEXT: Shade = Shade::new(
    Color32::from_rgb(255, 110, 140),
    Color32::from_rgb(200, 30, 90),
);
pub(crate) const WARNING_TEXT: Shade = Shade::new(
    Color32::from_rgb(240, 228, 66),
    Color32::from_rgb(140, 110, 0),
);
pub(crate) const GOOD_TEXT: Shade = Shade::new(
    Color32::from_rgb(120, 190, 255),
    Color32::from_rgb(0, 90, 200),
);

/// 行が言っていること（M6.481）: 問題、注意、よい結果。色と、形の違う印で示す。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// 問題（丸に ×）。
    Error,
    /// 注意（三角に !）。
    Warning,
    /// よい結果（丸に ✓）。
    Good,
}

impl Status {
    /// 今の表示での色。
    pub(crate) fn color(self) -> Color32 {
        match self {
            Status::Error => ERROR_TEXT.get(),
            Status::Warning => WARNING_TEXT.get(),
            Status::Good => GOOD_TEXT.get(),
        }
    }

    /// プロジェクトのチェックの重さ（"error"、"warning"）が言う状態。ほかは `None`（ただの知らせ）。
    pub(crate) fn of_severity(severity: &str) -> Option<Status> {
        let lower = severity.to_lowercase();
        if lower.contains("error") {
            Some(Status::Error)
        } else if lower.contains("warn") {
            Some(Status::Warning)
        } else {
            None
        }
    }
}

/// `status` の印を `rect` に描く（DiscCraft の図形）: 問題は丸に ×、注意は三角に !、よい結果は丸に ✓。中の線は、
/// 印の色が明るければ暗く、暗ければ白く。
pub(crate) fn paint_status_mark(painter: &egui::Painter, rect: egui::Rect, status: Status) {
    let color = status.color();
    let luma = 0.2126 * f32::from(color.r())
        + 0.7152 * f32::from(color.g())
        + 0.0722 * f32::from(color.b());
    let ink = if luma > 140.0 {
        Color32::from_gray(30)
    } else {
        Color32::WHITE
    };
    let ink_stroke = Stroke::new(1.5, ink);
    let c = rect.center();
    let r = rect.width().min(rect.height()) / 2.0;
    match status {
        Status::Error => {
            painter.circle_filled(c, r, color);
            let d = r * 0.42;
            painter.line_segment([c + egui::vec2(-d, -d), c + egui::vec2(d, d)], ink_stroke);
            painter.line_segment([c + egui::vec2(-d, d), c + egui::vec2(d, -d)], ink_stroke);
        }
        Status::Warning => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(c.x, c.y - r),
                    egui::pos2(c.x + r, c.y + r * 0.85),
                    egui::pos2(c.x - r, c.y + r * 0.85),
                ],
                color,
                Stroke::NONE,
            ));
            painter.line_segment(
                [
                    egui::pos2(c.x, c.y - r * 0.35),
                    egui::pos2(c.x, c.y + r * 0.25),
                ],
                ink_stroke,
            );
            painter.circle_filled(egui::pos2(c.x, c.y + r * 0.58), 0.9, ink);
        }
        Status::Good => {
            painter.circle_filled(c, r, color);
            painter.add(egui::Shape::line(
                vec![
                    c + egui::vec2(-r * 0.45, 0.0),
                    c + egui::vec2(-r * 0.1, r * 0.38),
                    c + egui::vec2(r * 0.5, -r * 0.38),
                ],
                ink_stroke,
            ));
        }
    }
}

/// `status` の印のあとに、その色の文字（M6.481。色が見分けにくくても印の形で分かる）。
pub(crate) fn status_label(
    ui: &mut egui::Ui,
    status: Status,
    text: impl Into<egui::RichText>,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let side = ui.text_style_height(&TextStyle::Body).clamp(10.0, 16.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), Sense::hover());
        paint_status_mark(ui.painter(), rect, status);
        ui.colored_label(status.color(), text)
    })
    .inner
}
/// パネルの間の溝（Encore の明るい表示: 102）。
pub(crate) const GUTTER: Shade = Shade::gray(16, 102);
/// パネルの地（Encore: 64 / 242）。
pub(crate) const PANEL: Shade = Shade::gray(60, 242);
/// タブの帯（Encore: 49 / 211）。
pub(crate) const TAB_STRIP: Shade = Shade::gray(43, 211);
/// 表の列見出しの帯（一覧の見出しのグラデーションの描けない表で）。
pub(crate) const HEADER: Shade = Shade::gray(76, 226);
/// 一覧の見出し（Encore のプロジェクトパネル、M6.468）：上から下へ 98 から 64、上の線 35、下の線 20。
const HEADER_TOP: Shade = Shade::gray(98, 250);
const HEADER_BOTTOM: Shade = Shade::gray(64, 226);
const HEADER_LINE_TOP: Shade = Shade::gray(35, 190);
const HEADER_LINE_BOTTOM: Shade = Shade::gray(20, 160);
/// 見出しの列の境目の溝（Encore: 明 87・暗 33・明 87）。
const GROOVE_LIGHT: Shade = Shade::gray(87, 251);
const GROOVE_DARK: Shade = Shade::gray(33, 170);
/// 表の行の下の区切り線（Encore: 64 の地に 59。1 行おきの縞はない）。
const ROW_LINE: Shade = Shade::gray(59, 222);
/// ライブラリの一覧の箱の線（Encore: パネルの地に 6 の線。M6.485）。
pub(crate) const LIST_EDGE: Shade = Shade::gray(6, 150);
/// ライブラリの一覧の行の間の線（Encore: 64 の地に 54）。
pub(crate) const LIST_ROW_LINE: Shade = Shade::gray(50, 222);
/// 並べ替えの印（▲ / ▼）。
const SORT_MARK: Shade = Shade::gray(200, 70);
/// 表の行の間（Encore: 行は 19 おき。18 のセルと 1 の間）。
const ROW_GAP: f32 = 1.0;
/// 入力欄やキャンバスの外側。
const FIELD: Shade = Shade::gray(38, 255);
/// ダイアログとポップアップの地。
const WINDOW: Shade = Shade::gray(66, 236);
/// 枠線。
pub(crate) const BORDER: Shade = Shade::gray(28, 165);
/// 操作中のパネルを囲む線（Encore で測った色。M6.474）。
pub(crate) const FOCUS_OUTLINE: Shade = Shade::same(Color32::from_rgb(210, 154, 0));
/// マウスが乗った道具やモニターの操作バーのボタンのアイコン（Encore で測った金色。M6.476。明るい地では濃く）。
pub(crate) const HOVER_ICON: Shade = Shade::new(
    Color32::from_rgb(188, 145, 36),
    Color32::from_rgb(156, 104, 0),
);
/// モニターの操作バーのアイコン（Encore: 187）。
pub(crate) const BAR_ICON: Shade = Shade::gray(187, 70);
/// マウスが乗ったボタンの箱（Encore: 縁 19、内側の上の明るい線 111、地は上 90 から下 67）。
const HOVER_EDGE: Shade = Shade::gray(19, 130);
const HOVER_SHINE: Shade = Shade::gray(111, 255);
const HOVER_TOP: Shade = Shade::gray(90, 252);
const HOVER_BOTTOM: Shade = Shade::gray(67, 222);
/// 選んでいる道具の、くぼんだ箱（Encore: 地 46、縁 6、下に明るい線 78）。
const SUNKEN: Shade = Shade::gray(46, 205);
const SUNKEN_EDGE: Shade = Shade::gray(6, 120);
/// 文字。
pub(crate) const TEXT: Shade = Shade::gray(216, 34);
/// 見出しや選択中のタブの文字。
pub(crate) const TEXT_STRONG: Shade = Shade::gray(242, 0);
/// 選ばれていないタブなど、控えめな文字。
pub(crate) const TEXT_WEAK: Shade = Shade::gray(156, 105);
/// 選択の地。
const SELECTION: Shade = Shade::new(
    Color32::from_rgb(50, 96, 148),
    Color32::from_rgb(164, 200, 240),
);
/// 表の選択中の行の地（Encore: 青ではなく 79 の灰色。M6.468。明るい表示は 217）。
pub(crate) const SELECTION_ROW: Shade = Shade::gray(79, 214);

/// タブの帯の高さ。
pub(crate) const TAB_HEIGHT: f32 = 24.0;
/// パネルの内側の余白（左右と下。上はタブの帯がパネルの縁に付くように 0）。
pub(crate) const GROUP_MARGIN: i8 = 6;
/// 隣のパネルとの間の溝の半分の幅。
const GUTTER_HALF: i8 = 1;

/// 配色と文字の大きさを設定する（暗い表示と明るい表示の両方）。OS の明るさには従わず、環境設定に保存した明るさ
/// （初めは暗い表示）にする。
pub fn apply_theme(ctx: &Context) {
    set_appearance(ctx, crate::workspace::load_appearance());
    for theme in [Theme::Dark, Theme::Light] {
        apply_style(ctx, theme);
    }
    ctx.replace_widget_theme::<TextEditStyle>(EncoreFields);
    ctx.replace_widget_theme::<ButtonStyle>(EncoreButtons);
}

/// `theme` の表示の配色と文字の大きさ。
fn apply_style(ctx: &Context, theme: Theme) {
    ctx.style_mut_of(theme, |style| {
        style.visuals = visuals(theme == Theme::Light);
        // The other Craft apps' size (M9.68: 12 px; Encore's Tahoma at 11 px until then).
        let body = crate::fonts::body_size();
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(body - 1.5, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(body, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(body, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(body + 3.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(body, FontFamily::Monospace),
            ),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(7.0, 3.0);
        style.spacing.button_padding = egui::vec2(7.0, 2.0);
        style.spacing.interact_size.y = 18.0;
    });
}

/// The class of a combo box that is a link field (End Action, First Play…): Encore shows those as light fields like
/// its text fields (M6.466).
pub(crate) const LINK_FIELD: egui::class::ClassName =
    egui::class::ClassName::from_static("disccraft::link_field");

/// The class of the button that answers a dialog by default (Yes, OK…): drawn as the other Craft apps draw it
/// (M6.510, PhotoCraft's Unsaved Changes): round ends, filled with the Craft apps' blue, white text.
pub(crate) const DIALOG_DEFAULT: egui::class::ClassName =
    egui::class::ClassName::from_static("disccraft::dialog_default");
/// The class of a dialog's other answers (No, Cancel…): round ends, an outline (M6.510).
pub(crate) const DIALOG_BUTTON: egui::class::ClassName =
    egui::class::ClassName::from_static("disccraft::dialog_button");

/// The Craft apps' blue of a default button (measured on PhotoCraft's Unsaved Changes: 55, 142, 240), lighter
/// under the pointer and darker pressed.
const CRAFT_BLUE: [Color32; 3] = [
    Color32::from_rgb(55, 142, 240),
    Color32::from_rgb(84, 160, 244),
    Color32::from_rgb(38, 118, 214),
];
/// The outline of a dialog's other buttons (light on the dark look, as PhotoCraft's; mid grey on the light one).
const DIALOG_OUTLINE: Shade = Shade::gray(205, 128);
/// What a dialog's other button is filled with under the pointer and pressed (a veil over the dialog's ground).
/// (Premultiplied: white at 20 and 36 of 255, black at 14 and 28.)
const DIALOG_VEIL: [Shade; 2] = [
    Shade::new(
        Color32::from_rgba_premultiplied(20, 20, 20, 20),
        Color32::from_rgba_premultiplied(0, 0, 0, 14),
    ),
    Shade::new(
        Color32::from_rgba_premultiplied(36, 36, 36, 36),
        Color32::from_rgba_premultiplied(0, 0, 0, 28),
    ),
];

/// A dialog's default answer (OK, Yes, Create, Close…): round ends, filled with the Craft apps' blue (M6.510).
pub(crate) fn default_button<'a>(text: impl egui::IntoAtoms<'a>) -> egui::Button<'a> {
    use egui::class::HasClasses as _;
    egui::Button::new(text).with_class(DIALOG_DEFAULT)
}

/// A dialog's other buttons (Cancel, No, Browse…): round ends, an outline (M6.510, M6.511 every dialog).
pub(crate) fn dialog_button<'a>(text: impl egui::IntoAtoms<'a>) -> egui::Button<'a> {
    use egui::class::HasClasses as _;
    egui::Button::new(text).with_class(DIALOG_BUTTON)
}

/// Draws a dialog's button ([`DIALOG_DEFAULT`] or [`DIALOG_BUTTON`], `default`) in `style`; `small` for one as high
/// as the text (`Button::small`, a row's Delete).
fn dialog_button_style(style: &mut ButtonStyle, state: WidgetState, default: bool, small: bool) {
    let frame = &mut style.atom_layout.frame;
    // As round as the button is high; wider and a little taller than the panels' buttons (the 1-pixel outline is
    // within the margin, as egui's own buttons have it).
    frame.corner_radius = CornerRadius::same(u8::MAX);
    frame.inner_margin = if small {
        Margin::symmetric(8, 0)
    } else {
        Margin::symmetric(13, 3)
    };
    let pick = |index: usize| match state {
        WidgetState::Hovered => index.min(1),
        WidgetState::Active => index.min(2),
        WidgetState::Inactive | WidgetState::Noninteractive => 0,
    };
    if default {
        let blue = CRAFT_BLUE.get(pick(2)).copied().unwrap_or(CRAFT_BLUE[0]);
        frame.fill = blue;
        frame.stroke = Stroke::new(1.0, blue);
        style.atom_layout.text_style.color = Color32::WHITE;
    } else {
        frame.fill = match pick(2) {
            0 => Color32::TRANSPARENT,
            step => DIALOG_VEIL
                .get(step.saturating_sub(1))
                .map_or(Color32::TRANSPARENT, |veil| veil.get()),
        };
        frame.stroke = Stroke::new(1.0, DIALOG_OUTLINE.get());
    }
}

/// Buttons as egui draws them, but a [`LINK_FIELD`] combo box as Encore's light field (a little lighter under the
/// pointer), and a dialog's answers as the other Craft apps draw them ([`DIALOG_DEFAULT`], [`DIALOG_BUTTON`]).
struct EncoreButtons;

/// A link field under the pointer.
const FIELD_HOVER: Shade = Shade::gray(172, 246);

impl StyleProvider<ButtonStyle> for EncoreButtons {
    fn style(&mut self, args: &StyleArgs<'_>) -> ButtonStyle {
        use egui::class::HasClasses as _;
        let mut style = StyleProvider::<ButtonStyle>::style(&mut DefaultStyle, args);
        let small = args.classes.has_class(&egui::Button::CLASS_SMALL);
        if args.classes.has_class(&DIALOG_DEFAULT) {
            dialog_button_style(&mut style, args.state, true, small);
        } else if args.classes.has_class(&DIALOG_BUTTON) {
            dialog_button_style(&mut style, args.state, false, small);
        }
        if args.classes.has_class(&LINK_FIELD) {
            style.atom_layout.frame.fill = if args.state == WidgetState::Hovered {
                FIELD_HOVER.get()
            } else {
                FIELD_LIGHT.get()
            };
            style.atom_layout.frame.stroke = Stroke::new(1.0, FIELD_EDGE);
            style.atom_layout.text_style.color = FIELD_TEXT;
        }
        style
    }
}

/// Encore's text fields (M6.465, measured on its Properties and Build panels): light grey (159) in a dark edge (6),
/// with black text. A read-only one stays as egui draws it (no ground, the panel's text colour).
struct EncoreFields;

/// A text field's ground, edge, text and hint (Encore: 159, 6 and black; white in a grey edge in the light look).
const FIELD_LIGHT: Shade = Shade::gray(159, 255);
const FIELD_EDGE: Shade = Shade::gray(6, 150);
const FIELD_TEXT: Color32 = Color32::BLACK;
const FIELD_HINT: Shade = Shade::gray(96, 140);

impl StyleProvider<TextEditStyle> for EncoreFields {
    fn style(&mut self, args: &StyleArgs<'_>) -> TextEditStyle {
        use egui::class::HasClasses as _;
        let mut style = StyleProvider::<TextEditStyle>::style(&mut DefaultStyle, args);
        if args.classes.has_class(&egui::TextEdit::CLASS_READ_ONLY) {
            return style;
        }
        style.atom_layout.frame.fill = FIELD_LIGHT.get();
        // While typing, the edge stays egui's (the selection colour) so the focus shows.
        if args.state != WidgetState::Active {
            style.atom_layout.frame.stroke = Stroke::new(1.0, FIELD_EDGE);
        }
        style.atom_layout.text_style.color = FIELD_TEXT;
        style.hint_text_color = FIELD_HINT.get();
        style.prefix_suffix_color = FIELD_TEXT;
        style
    }
}

/// egui's colours for the dark look (`light` false) or the light one.
fn visuals(light: bool) -> Visuals {
    let c = |shade: Shade| shade.pick(light);
    let mut visuals = if light {
        Visuals::light()
    } else {
        Visuals::dark()
    };
    visuals.panel_fill = c(PANEL);
    visuals.window_fill = c(WINDOW);
    visuals.window_stroke = Stroke::new(1.0, c(BORDER));
    visuals.extreme_bg_color = c(FIELD);
    visuals.faint_bg_color = c(Shade::gray(65, 236));
    visuals.code_bg_color = c(FIELD);
    visuals.override_text_color = None;
    visuals.selection.bg_fill = c(SELECTION);
    visuals.selection.stroke = Stroke::new(1.0, c(TEXT_STRONG));
    // The text cursor stands in Encore's light fields (M6.465).
    visuals.text_cursor.stroke.color = FIELD_TEXT;
    visuals.window_corner_radius = CornerRadius::same(4);
    visuals.menu_corner_radius = CornerRadius::same(3);

    let radius = CornerRadius::same(2);
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = c(PANEL);
    widgets.noninteractive.weak_bg_fill = c(PANEL);
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, c(BORDER));
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, c(TEXT));
    widgets.noninteractive.corner_radius = radius;
    for (state, fill, text) in [
        (&mut widgets.inactive, Shade::gray(76, 226), TEXT),
        (&mut widgets.hovered, Shade::gray(90, 236), TEXT_STRONG),
        (&mut widgets.active, Shade::gray(104, 212), TEXT_STRONG),
        (&mut widgets.open, Shade::gray(84, 230), TEXT_STRONG),
    ] {
        state.bg_fill = c(fill);
        state.weak_bg_fill = c(fill);
        state.bg_stroke = Stroke::new(1.0, c(BORDER));
        state.fg_stroke = Stroke::new(1.0, c(text));
        state.corner_radius = radius;
    }
    visuals
}

/// ツールバーとステータスバーの枠。
pub(crate) fn bar_frame() -> Frame {
    Frame::new()
        .fill(PANEL.get())
        .inner_margin(Margin::symmetric(GROUP_MARGIN, 3))
        .outer_margin(Margin::same(GUTTER_HALF))
}

/// 列見出しのある表。1 行目（見出し）に明るい帯、行は 19 おき（Encore、M6.468: 1 行おきの縞はない）。
pub(crate) fn table(id_salt: &str) -> egui::Grid {
    table_selecting_rows(id_salt, Vec::new())
}

/// 一覧（[`TableFill`] と組にする表）: 選択中の行（見出しを除いた 0 始まり）を行全体の選択の色で塗る。見出しは
/// [`TableFill`] が Encore のグラデーションで、行の区切り線は [`table_row`] が描く（M6.468）。
pub(crate) fn table_selecting(id_salt: &str, selected: Option<usize>) -> egui::Grid {
    egui::Grid::new(id_salt)
        .min_col_width(48.0)
        .spacing([7.0, ROW_GAP])
        .with_row_color(move |row, _style| match row.checked_sub(1) {
            Some(body) if Some(body) == selected => Some(SELECTION_ROW.get()),
            _ => None,
        })
}

/// [`table`] で、選んでいる行が複数あるもの（見出しを除いた 0 始まり。M6.425: チャプター再生リストの左の表）。
pub(crate) fn table_selecting_rows(
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    selected: Vec<usize>,
) -> egui::Grid {
    egui::Grid::new(id_salt)
        .min_col_width(48.0)
        .spacing([7.0, ROW_GAP])
        .with_row_color(move |row, _style| match row.checked_sub(1) {
            None => Some(HEADER.get()),
            Some(body) if selected.contains(&body) => Some(SELECTION_ROW.get()),
            Some(_) => None,
        })
}

/// The box round a tool or a monitor bar's button under the pointer (M6.476, the look measured on Encore's toolbar
/// and monitor bar): a dark edge, a light line inside along its top, a ground shaded a little lighter than the bar.
/// The icon on it is drawn in [`HOVER_ICON`].
pub(crate) fn hover_box(painter: &egui::Painter, rect: egui::Rect) {
    use egui::emath::GuiRounding as _;
    let rect = rect.round_to_pixels(painter.pixels_per_point());
    let inner = rect.shrink(1.0);
    painter.add(shaded(inner, HOVER_TOP.get(), HOVER_BOTTOM.get()));
    painter.hline(
        inner.x_range(),
        inner.min.y + 0.5,
        Stroke::new(1.0, HOVER_SHINE),
    );
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, HOVER_EDGE),
        egui::StrokeKind::Inside,
    );
}

/// The chosen tool's box (M6.476, Encore's toolbar): sunk into the bar, a dark ground in a near-black edge with a
/// light line under it.
pub(crate) fn sunken_box(painter: &egui::Painter, rect: egui::Rect) {
    use egui::emath::GuiRounding as _;
    let rect = rect.round_to_pixels(painter.pixels_per_point());
    painter.rect_filled(rect, 0.0, SUNKEN);
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, SUNKEN_EDGE),
        egui::StrokeKind::Inside,
    );
    painter.hline(
        rect.x_range(),
        rect.max.y + 0.5,
        Stroke::new(1.0, GROUP_LIGHT),
    );
}

/// The line between a bar's groups of buttons (Encore's monitor bar: a dark line with a light one beside it).
pub(crate) fn bar_separator(painter: &egui::Painter, x: f32, y: egui::Rangef) {
    let x = x.round();
    painter.vline(x + 0.5, y, Stroke::new(1.0, GROUP_DARK));
    painter.vline(x + 1.5, y, Stroke::new(1.0, GROUP_LIGHT));
}

/// A rectangle shaded from `top` down to `bottom`.
fn shaded(rect: egui::Rect, top: Color32, bottom: Color32) -> egui::Shape {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    egui::Shape::mesh(mesh)
}

/// The header row of a list `table` wide, its cells `height` high (Encore: shaded 98 to 64 with a dark line above and
/// below).
fn header_shapes(table: egui::Rect, height: f32) -> Vec<egui::Shape> {
    let body = egui::Rect::from_min_max(
        egui::pos2(table.min.x, table.min.y - 1.0),
        egui::pos2(table.max.x, table.min.y + height),
    );
    vec![
        shaded(body, HEADER_TOP.get(), HEADER_BOTTOM.get()),
        egui::Shape::hline(
            body.x_range(),
            body.min.y + 0.5,
            Stroke::new(1.0, HEADER_LINE_TOP),
        ),
        egui::Shape::hline(
            body.x_range(),
            body.max.y + 0.5,
            Stroke::new(1.0, HEADER_LINE_BOTTOM),
        ),
    ]
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

/// 表の見出しのセル。押すと並べ替えが変わる（並べ替えている列の ▲ / ▼ は [`resizable_header`] が列の右端に描く）。
pub(crate) fn sort_header(
    ui: &mut egui::Ui,
    sort: &mut TableSort,
    column: usize,
    label: &str,
) -> egui::Response {
    let response = ui
        .add(egui::Label::new(label).sense(Sense::click()))
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

/// 表の列の幅（見出しの右端をドラッグして決めたもの）。キーは「表:列」。ない列は中身に合わせる。
pub(crate) type ColumnWidths = std::collections::BTreeMap<String, f32>;
/// ドラッグで決められるいちばん狭い列の幅。
const COLUMN_MIN: f32 = 32.0;
/// いちばん広い列の幅。
const COLUMN_MAX: f32 = 2000.0;

/// 見出しの右端のつまみの操作。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ColumnEdge {
    /// ドラッグした: 列の幅をこの値に。
    Width(f32),
    /// ダブルクリックした: 決めた幅を消して初めの幅（中身に合わせる幅か、プロジェクトの表なら Encore の幅）に戻す。
    Fit,
}

/// 幅を変えられる列の見出し（Encore の表と同じく、見出しの右端の境目をドラッグすると列の幅が変わり、
/// ダブルクリックで中身に合わせた幅に戻る）。押すと [`sort_header`] と同じく並べ替える。`width` は決めた幅
/// （`None` は中身に合わせる）。見出しの応答と、このフレームでのつまみの操作を返す。
pub(crate) fn resizable_header(
    ui: &mut egui::Ui,
    sort: &mut TableSort,
    column: usize,
    label: &str,
    width: Option<f32>,
) -> (egui::Response, Option<ColumnEdge>) {
    let left = ui.cursor().min.x;
    // 中身に合わせる列は、前のフレームで表が決めた列の幅（グリッドのセルの幅）から始める。
    let current = width.unwrap_or_else(|| ui.available_width());
    let (response, cell) = fixed_cell(ui, width, |ui| sort_header(ui, sort, column, label));
    let gap = ui.spacing().item_spacing.x;
    let right = left + current.max(cell.width());
    let edge_x = right + gap / 2.0;
    let handle = egui::Rect::from_min_max(
        egui::pos2(edge_x - 4.0, cell.top()),
        egui::pos2(edge_x + 4.0, cell.bottom()),
    );
    let edge = ui.interact(
        handle,
        ui.make_persistent_id(("column_edge", column)),
        Sense::click_and_drag(),
    );
    // Encore: a groove between columns (light, dark, light); it lights up under the pointer.
    let middle = if edge.hovered() || edge.dragged() {
        TEXT
    } else {
        GROOVE_DARK
    };
    let edge_x = edge_x.round();
    for (x, color) in [
        (edge_x - 1.0, GROOVE_LIGHT),
        (edge_x, middle),
        (edge_x + 1.0, GROOVE_LIGHT),
    ] {
        ui.painter()
            .vline(x + 0.5, cell.y_range(), Stroke::new(1.0, color));
    }
    // The sorted column's mark at its right end (Encore: 名前 ▲).
    if sort.column == column {
        let center = egui::pos2(right - 6.0, cell.center().y);
        let (dir, size) = (
            if sort.ascending { -1.0 } else { 1.0 },
            egui::vec2(9.0, 5.0),
        );
        let _ = ui.painter().add(crate::scroll_bars::arrow(
            center,
            egui::vec2(0.0, dir),
            size,
            SORT_MARK.get(),
        ));
    }
    if edge.hovered() || edge.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let action = if edge.double_clicked() {
        Some(ColumnEdge::Fit)
    } else if edge.dragged() && edge.drag_delta().x != 0.0 {
        Some(ColumnEdge::Width(
            (current + edge.drag_delta().x).clamp(COLUMN_MIN, COLUMN_MAX),
        ))
    } else {
        None
    };
    (response, action)
}

/// 1 つの表の、幅を変えられる列（1 フレームぶん）: 決めた幅と、表を描いたあとで当てる見出しの右端の操作。
pub(crate) struct Columns {
    widths: Vec<Option<f32>>,
    /// このフレームの見出しの右端の操作（列の番号と操作）。
    pub(crate) edges: Vec<(usize, ColumnEdge)>,
}

impl Columns {
    /// 列ごとの決めた幅（`None` は中身に合わせる）。
    pub(crate) fn new(widths: Vec<Option<f32>>) -> Self {
        Self {
            widths,
            edges: Vec::new(),
        }
    }

    fn width(&self, column: usize) -> Option<f32> {
        self.widths.get(column).copied().flatten()
    }

    /// 列 `column` の見出し（[`resizable_header`]）。
    pub(crate) fn header(
        &mut self,
        ui: &mut egui::Ui,
        sort: &mut TableSort,
        column: usize,
        label: &str,
    ) -> egui::Response {
        let (response, edge) = resizable_header(ui, sort, column, label, self.width(column));
        if let Some(edge) = edge {
            self.edges.push((column, edge));
        }
        response
    }

    /// 列 `column` のセルに文字を置き、セルの範囲を返す。
    pub(crate) fn label(
        &self,
        ui: &mut egui::Ui,
        column: usize,
        text: impl Into<egui::WidgetText>,
    ) -> egui::Rect {
        fixed_cell(ui, self.width(column), |ui| ui.label(text)).1
    }

    /// 列 `column` のセルに `add` で描き、その結果とセルの範囲を返す。
    pub(crate) fn cell<R>(
        &self,
        ui: &mut egui::Ui,
        column: usize,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> (R, egui::Rect) {
        fixed_cell(ui, self.width(column), add)
    }
}

/// 表のセル。`width` があればその幅ちょうどにし、入りきらない文字は「…」で切る。`None` は中身の大きさ。
/// `add` の結果と、セルの範囲を返す。
pub(crate) fn fixed_cell<R>(
    ui: &mut egui::Ui,
    width: Option<f32>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, egui::Rect) {
    match width {
        None => {
            let response = ui.scope(add);
            (response.inner, response.response.rect)
        }
        Some(width) => {
            let height = ui.spacing().interact_size.y;
            let response = ui.allocate_ui_with_layout(
                egui::vec2(width, height),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_width(width);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    add(ui)
                },
            );
            (response.inner, response.response.rect)
        }
    }
}

/// How many of a cell's first characters Encore keeps when it shortens the text (M6.469: "SUMMER ...2014 20th
/// Anniversary", "トランスコード...ません").
const ELIDE_HEAD: usize = 7;
/// The room Encore keeps clear at the right end of a cell whose text it shortens.
const ELIDE_MARGIN: f32 = 3.0;

/// How Encore shortens a cell's text that does not fit in `room`, given each character's width and the width of
/// "...": its first [`ELIDE_HEAD`] characters, "..." and as many of its last characters as fit; when those leave no
/// room for any of the end, "..." and the end alone ("...いません"). `None` when the text fits whole; `(head, tail)`
/// character counts otherwise ((0, 0): only "...").
pub(crate) fn elide_parts(widths: &[f32], dots: f32, room: f32) -> Option<(usize, usize)> {
    if room.is_nan() {
        return Some((0, 0));
    }
    let total: f32 = widths.iter().sum();
    if total <= room {
        return None;
    }
    let count = widths.len();
    for head in [ELIDE_HEAD.min(count), 0] {
        let head_width: f32 = widths.iter().take(head).sum();
        let left = room - head_width - dots;
        if left < 0.0 {
            continue;
        }
        let mut tail = 0;
        let mut tail_width = 0.0;
        for width in widths.iter().skip(head).rev() {
            if tail_width + width > left {
                break;
            }
            tail_width += width;
            tail += 1;
        }
        if head == 0 || tail > 0 {
            return Some((head, tail));
        }
    }
    Some((0, 0))
}

/// A label in the room left in its cell, shortened as Encore shortens a list's text when it does not fit (see
/// [`elide_parts`]); the whole text shows under the pointer then.
pub(crate) fn elided_label(ui: &mut egui::Ui, text: &str) -> egui::Response {
    // Encore keeps a few pixels clear at a cell's right end.
    let room = ui.available_width() - ELIDE_MARGIN;
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), TEXT.get());
    if galley.size().x <= room {
        return ui.label(text);
    }
    let glyphs: Vec<(char, f32)> = galley
        .rows
        .iter()
        .flat_map(|row| {
            row.glyphs
                .iter()
                .map(|glyph| (glyph.chr, glyph.advance_width))
        })
        .collect();
    let dots = ui
        .painter()
        .layout_no_wrap("...".to_owned(), font, TEXT.get())
        .size()
        .x;
    let widths: Vec<f32> = glyphs.iter().map(|(_, width)| *width).collect();
    let Some((head, tail)) = elide_parts(&widths, dots, room) else {
        return ui.label(text);
    };
    let mut shown: String = glyphs.iter().take(head).map(|(c, _)| *c).collect();
    shown.push_str("...");
    let skip = glyphs.len().saturating_sub(tail);
    shown.extend(glyphs.iter().skip(skip).map(|(c, _)| *c));
    ui.label(shown).on_hover_text(text)
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
    /// 見出しの帯を表の後ろに描く場所（表を描く前に取り、描いたあとで大きさが分かってから埋める）。
    header: egui::layers::ShapeIdx,
}

impl TableFill {
    pub(crate) fn new(ui: &egui::Ui, id_salt: &str) -> Self {
        let key = egui::Id::unique("table_fill").with(id_salt);
        let (total, filler) = ui
            .data(|data| data.get_temp::<(f32, f32)>(key))
            .unwrap_or((0.0, 0.0));
        let columns = (total - filler).max(0.0);
        let filler = (ui.available_width() - columns).max(0.0);
        let header = ui.painter().add(egui::Shape::Noop);
        Self {
            key,
            filler,
            columns,
            header,
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

    /// 表を描いたあとで、見出しの帯を表の後ろに描き、表の幅を覚える（変わったら描き直す）。
    pub(crate) fn store(&self, ui: &egui::Ui, table: egui::Rect) {
        let gap = ui.spacing().item_spacing.x / 2.0;
        let wide = egui::Rect::from_min_max(egui::pos2(table.min.x - gap, table.min.y), table.max);
        ui.painter().set(
            self.header,
            egui::Shape::Vec(header_shapes(wide, ui.spacing().interact_size.y)),
        );
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

/// ダイアログの本体: ウィンドウの高さから `reserved`（見出しとボタンの行の分）を引いた高さまで。それより高い中身は
/// スクロールする（低い解像度や大きな表示倍率の画面でも、ボタンが画面の外に出ない。M9.37）。
pub(crate) fn dialog_body(
    ui: &mut egui::Ui,
    id: &str,
    reserved: f32,
    add: impl FnOnce(&mut egui::Ui),
) {
    let room = (ui.ctx().content_rect().height() - reserved).max(160.0);
    let _ = egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(room)
        .show(ui, add);
}

/// 枠で囲んだグループ（Encore のビルドパネルの「ソース」「保存先」などと同じ）。見出しを押すと中身をたたむ。
/// Encore と同じく、見出し（▼ ソース）は枠の上、枠は中身を彫り込みの線で囲み（暗い線と、1 ずらした明るい線。
/// 塗りはない。M6.471）、横幅いっぱいに広げ、グループの間を少し空ける。
pub(crate) fn group_box(
    ui: &mut egui::Ui,
    id_salt: &str,
    title: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    let id = ui.make_persistent_id(("group_box", id_salt));
    let mut open = ui.data_mut(|data| *data.get_persisted_mut_or(id, true));
    // The heading: a triangle (down while open) and the title, in from the frame's edge (Encore: ▼ 24 in).
    ui.horizontal(|ui| {
        ui.add_space(GROUP_INSET + 6.0);
        let (mark, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), Sense::hover());
        let direction = if open {
            egui::vec2(0.0, 1.0)
        } else {
            egui::vec2(1.0, 0.0)
        };
        let size = if open {
            egui::vec2(8.0, 5.0)
        } else {
            egui::vec2(5.0, 8.0)
        };
        let _ = ui.painter().add(crate::scroll_bars::arrow(
            mark.center(),
            direction,
            size,
            ui.visuals().text_color(),
        ));
        let title = ui.add(egui::Label::new(title).sense(Sense::click()));
        let mark = ui.interact(mark, id.with("mark"), Sense::click());
        if title.clicked() || mark.clicked() {
            open = !open;
            ui.data_mut(|data| data.insert_persisted(id, open));
        }
    });
    if open {
        let inner = Frame::new()
            .outer_margin(Margin {
                left: GROUP_INSET as i8,
                right: GROUP_INSET as i8,
                top: 0,
                bottom: 0,
            })
            .inner_margin(Margin {
                left: 10,
                right: 10,
                top: 6,
                bottom: 6,
            })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                body(ui);
            });
        let rect = inner.response.rect.shrink2(egui::vec2(GROUP_INSET, 0.0));
        let painter = ui.painter();
        for (shift, color) in [(1.0, GROUP_LIGHT), (0.0, GROUP_DARK)] {
            let _ = painter.rect_stroke(
                rect.translate(egui::vec2(shift, shift)),
                2.0,
                Stroke::new(1.0, color),
                egui::StrokeKind::Inside,
            );
        }
    }
    ui.add_space(4.0);
}

/// How far a group's frame is in from the panel's sides (Encore: 9).
const GROUP_INSET: f32 = 9.0;

/// A group's etched edge: a dark line and a light one a pixel to the lower right (Encore: 29 and 78).
const GROUP_DARK: Shade = Shade::gray(29, 169);
const GROUP_LIGHT: Shade = Shade::gray(78, 251);

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
    let gap = ui.spacing().item_spacing.x;
    let rect = first
        .union(last)
        .expand2(egui::vec2(gap * 0.5, ROW_GAP * 0.5));
    // Encore: a thin line under each row (no stripes).
    ui.painter()
        .hline(rect.x_range(), rect.max.y, Stroke::new(1.0, ROW_LINE));
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

#[cfg(test)]
mod tests {
    use super::{ERROR_TEXT, GOOD_TEXT, Shade, Status, WARNING_TEXT, elide_parts};

    /// A colour as seen with protanopia, deuteranopia or tritanopia (Machado, Oliveira and Fernandes 2009, severity
    /// 1, on linear RGB).
    fn simulated(color: [u8; 3], matrix: [[f64; 3]; 3]) -> [f64; 3] {
        let lin = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let l = color.map(lin);
        let mut out = [0.0; 3];
        for (row, value) in matrix.iter().zip(out.iter_mut()) {
            *value = (row[0] * l[0] + row[1] * l[1] + row[2] * l[2]).clamp(0.0, 1.0);
        }
        out
    }

    /// CIELAB of a linear RGB colour (D65).
    fn lab([r, g, b]: [f64; 3]) -> [f64; 3] {
        let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
        let f = |t: f64| {
            if t > 0.008856 {
                t.cbrt()
            } else {
                7.787 * t + 16.0 / 116.0
            }
        };
        [
            116.0 * f(y) - 16.0,
            500.0 * (f(x) - f(y)),
            200.0 * (f(y) - f(z)),
        ]
    }

    /// The error, warning and good colours stay far apart (ΔE 30 or more) for each kind of colour vision, in both
    /// looks (M6.481).
    #[test]
    fn the_status_colours_read_for_every_kind_of_colour_vision() {
        const KINDS: [(&str, [[f64; 3]; 3]); 3] = [
            (
                "protanopia",
                [
                    [0.152286, 1.052583, -0.204868],
                    [0.114503, 0.786281, 0.099216],
                    [-0.003882, -0.048116, 1.051998],
                ],
            ),
            (
                "deuteranopia",
                [
                    [0.367322, 0.860646, -0.227968],
                    [0.280085, 0.672501, 0.047413],
                    [-0.011820, 0.042940, 0.968881],
                ],
            ),
            (
                "tritanopia",
                [
                    [1.255528, -0.076749, -0.178779],
                    [-0.078411, 0.930809, 0.147602],
                    [0.004733, 0.691367, 0.303900],
                ],
            ),
        ];
        let rgb = |shade: Shade, light: bool| {
            let c = shade.pick(light);
            [c.r(), c.g(), c.b()]
        };
        for light in [false, true] {
            for (a, b) in [(ERROR_TEXT, WARNING_TEXT), (ERROR_TEXT, GOOD_TEXT)] {
                for (kind, matrix) in KINDS {
                    let (pa, pb) = (
                        lab(simulated(rgb(a, light), matrix)),
                        lab(simulated(rgb(b, light), matrix)),
                    );
                    let distance = pa
                        .iter()
                        .zip(pb.iter())
                        .map(|(x, y)| (x - y).powi(2))
                        .sum::<f64>()
                        .sqrt();
                    assert!(
                        distance >= 30.0,
                        "{kind}, light {light}: {:?} and {:?} are only {distance:.1} apart",
                        rgb(a, light),
                        rgb(b, light)
                    );
                }
            }
        }
        // The check's severities.
        assert_eq!(Status::of_severity("Error"), Some(Status::Error));
        assert_eq!(Status::of_severity("warning"), Some(Status::Warning));
        assert_eq!(Status::of_severity("info"), None);
    }

    #[test]
    fn text_is_shortened_as_encore_does() {
        // Fits: shown whole.
        assert_eq!(elide_parts(&[5.0; 10], 9.0, 50.0), None);
        // The first 7, "..." and as much of the end as fits: 35 + 9 leaves 16, three of the end.
        assert_eq!(elide_parts(&[5.0; 20], 9.0, 60.0), Some((7, 3)));
        // No room for any of the end after the first 7: "..." and the end alone.
        assert_eq!(elide_parts(&[10.0; 14], 9.0, 79.0), Some((0, 7)));
        // Not even "..." and one character: only "...".
        assert_eq!(elide_parts(&[10.0; 3], 9.0, 12.0), Some((0, 0)));
        assert_eq!(elide_parts(&[10.0; 3], 9.0, f32::NAN), Some((0, 0)));
        // Shorter than 7: the head is what there is.
        assert_eq!(elide_parts(&[10.0; 5], 9.0, 49.0), Some((0, 4)));
    }
}
