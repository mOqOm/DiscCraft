//! メニューのボタンの編集: 複数選択、切り取り / コピー / 貼り付け / 複製、すべてを選択、矢印キーでの移動、
//! オブジェクト ▸ 整列 / 均等配置（[docs/ui-parity.md](../../../docs/ui-parity.md) §3.1）。
//!
//! どの操作も engine のコマンド 1 回で、1 回の取り消しになる。

use crate::DiscCraftApp;
use crate::app::Selection;
use crate::i18n::tl;
use crate::workspace::CenterTab;

use disccraft_project::{
    Align, Arrange, Axis, Link, Menu, MenuId, Rect, arranged_order, bounds, frame_size,
    free_offset, moved_order,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Shift を押しながら矢印キーで動かす量（押さないときは 1）。
const BIG_NUDGE: i32 = 10;

/// クリップボードに置くボタン（`menu.pasteButtons` の要素と同じ形）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ClipButton {
    /// ボタンテキスト。
    pub(crate) label: String,
    /// 位置と大きさ。
    pub(crate) rect: Rect,
    /// リンク。
    #[serde(default)]
    pub(crate) target: Link,
    /// 自動アクティベート。
    #[serde(default, rename = "autoActivate")]
    pub(crate) auto_activate: bool,
}

/// システムのクリップボードに書く JSON。別の DiscCraft のウィンドウにも貼り付けられる。
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardText {
    disccraft_buttons: Vec<ClipButton>,
}

/// クリップボードの文字列が DiscCraft のボタンなら取り出す。
pub(crate) fn parse_clipboard(text: &str) -> Option<Vec<ClipButton>> {
    serde_json::from_str::<ClipboardText>(text)
        .ok()
        .map(|clip| clip.disccraft_buttons)
        .filter(|buttons| !buttons.is_empty())
}

/// クリップボードに書く文字列。
pub(crate) fn clipboard_text(buttons: &[ClipButton]) -> Option<String> {
    serde_json::to_string(&ClipboardText {
        disccraft_buttons: buttons.to_vec(),
    })
    .ok()
}

/// 貼り付ける位置。DVD のボタンは重ねられないので、元の位置がふさがっていればいちばん近い空いている場所に
/// まとめて置く（並びは崩さない）。置けるところがなければ `None`。
pub(crate) fn paste_positions(
    clip: &[ClipButton],
    existing: &[Rect],
    frame: (u16, u16),
) -> Option<Vec<ClipButton>> {
    let group: Vec<Rect> = clip.iter().map(|button| button.rect).collect();
    let (dx, dy) = free_offset(&group, existing, frame)?;
    let indexed: Vec<(usize, Rect)> = group.into_iter().enumerate().collect();
    let placed = moved(&indexed, dx, dy, frame);
    Some(
        clip.iter()
            .zip(placed)
            .map(|(button, (_, rect))| ClipButton {
                rect,
                ..button.clone()
            })
            .collect(),
    )
}

/// まとめて `dx`, `dy` だけ動かした矩形。全体が枠からはみ出す手前で止める（並びは崩さない）。
pub(crate) fn moved(
    rects: &[(usize, Rect)],
    dx: i32,
    dy: i32,
    frame: (u16, u16),
) -> Vec<(usize, Rect)> {
    let only: Vec<Rect> = rects.iter().map(|(_, rect)| *rect).collect();
    let Some(group) = bounds(&only) else {
        return Vec::new();
    };
    let dx = dx.clamp(
        -i32::from(group.x),
        (i32::from(frame.0) - i32::from(group.x) - i32::from(group.w)).max(0),
    );
    let dy = dy.clamp(
        -i32::from(group.y),
        (i32::from(frame.1) - i32::from(group.y) - i32::from(group.h)).max(0),
    );
    rects
        .iter()
        .map(|(index, rect)| {
            let x = u16::try_from(i32::from(rect.x).saturating_add(dx)).unwrap_or(rect.x);
            let y = u16::try_from(i32::from(rect.y).saturating_add(dy)).unwrap_or(rect.y);
            (*index, Rect { x, y, ..*rect })
        })
        .collect()
}

/// 選択を足し引きする。あれば外し、なければ加える（加えたものが主になる）。
pub(crate) fn toggled(selection: &[usize], index: usize) -> Vec<usize> {
    if selection.contains(&index) {
        selection
            .iter()
            .copied()
            .filter(|selected| *selected != index)
            .collect()
    } else {
        let mut out = selection.to_vec();
        out.push(index);
        out
    }
}

impl DiscCraftApp {
    /// 主に選んでいるボタン（最後に選んだもの）。プロパティパネルはこのボタンのシートを出す。
    pub(crate) fn primary_button(&self) -> Option<usize> {
        self.selected_buttons.last().copied()
    }

    /// `index` のボタンを選んでいるか。
    pub(crate) fn button_selected(&self, index: usize) -> bool {
        self.selected_buttons.contains(&index)
    }

    /// 編集の対象になるメニュー（メニュービューアが前面で、そのメニューを選んでいるとき）。
    pub(crate) fn edit_menu_target(&self) -> Option<MenuId> {
        match self.selection {
            Selection::Menu(id) if self.tab == CenterTab::Menu => Some(id),
            _ => None,
        }
    }

    /// ボタンを 1 つだけ選ぶ。
    pub(crate) fn select_only_button(&mut self, menu: MenuId, index: usize) {
        self.set_button_selection(menu, vec![index]);
    }

    /// Shift / Ctrl を押しながらのクリック: 選択に加える、選んでいれば外す。
    pub(crate) fn toggle_button(&mut self, menu: MenuId, index: usize) {
        let selection = if self.selection == Selection::Menu(menu) {
            toggled(&self.selected_buttons, index)
        } else {
            vec![index]
        };
        self.set_button_selection(menu, selection);
    }

    /// メニューを選択にして、ボタンの選択を `indexes` にする（最後が主）。
    pub(crate) fn set_button_selection(&mut self, menu: MenuId, indexes: Vec<usize>) {
        // ビューアのメニューを選択にして、プロパティにボタンのシートを出す。select はボタンの選択を外す。
        self.select(Selection::Menu(menu));
        self.selected_buttons = indexes;
        if let Some(index) = self.primary_button() {
            let label = self
                .session
                .project
                .menu(menu)
                .and_then(|menu| menu.buttons.get(index))
                .map(|button| button.label.clone())
                .unwrap_or_default();
            self.button_label_menu = Some(menu);
            self.button_label_button = Some(index);
            self.button_label = label;
        }
    }

    /// 選んでいるボタン（番号順）を、クリップボードの形で。
    fn selected_clip(&self, menu: MenuId) -> Vec<ClipButton> {
        let Some(menu) = self.session.project.menu(menu) else {
            return Vec::new();
        };
        let mut indexes = self.selected_buttons.clone();
        indexes.sort_unstable();
        indexes
            .iter()
            .filter_map(|index| menu.buttons.get(*index))
            .map(|button| ClipButton {
                label: button.label.clone(),
                rect: button.rect,
                target: button.link.clone(),
                auto_activate: button.auto_activate,
            })
            .collect()
    }

    /// 編集 ▸ コピー。DiscCraft の中のクリップボードと、システムのクリップボード（JSON）の両方に置く。
    pub(crate) fn copy_buttons(&mut self, ctx: &egui::Context) -> bool {
        let Some(menu) = self.edit_menu_target() else {
            return false;
        };
        let clip = self.selected_clip(menu);
        if clip.is_empty() {
            return false;
        }
        if let Some(text) = clipboard_text(&clip) {
            ctx.copy_text(text);
        }
        self.button_clipboard = clip;
        true
    }

    /// 編集 ▸ 切り取り。
    pub(crate) fn cut_buttons(&mut self, ctx: &egui::Context) {
        if self.copy_buttons(ctx) {
            self.remove_selected_buttons();
        }
    }

    /// 編集 ▸ 貼り付け。Ctrl+V でクリップボードに DiscCraft のボタンがあればそれを、なければ中のクリップボードを
    /// 貼る。貼ったボタンを選ぶ。
    pub(crate) fn paste_buttons(&mut self) {
        let pasted = self
            .pasted_text
            .take()
            .and_then(|text| parse_clipboard(&text));
        let clip = pasted.unwrap_or_else(|| self.button_clipboard.clone());
        if clip.is_empty() {
            return;
        }
        if let Some(menu) = self.edit_menu_target() {
            self.add_button_copies(menu, &clip);
        }
    }

    /// 編集 ▸ 複製。選んでいるボタンを少しずらして複製する。
    pub(crate) fn duplicate_buttons(&mut self) {
        let Some(menu) = self.edit_menu_target() else {
            return;
        };
        let clip = self.selected_clip(menu);
        if !clip.is_empty() {
            self.add_button_copies(menu, &clip);
        }
    }

    /// `clip` を `menu` の最後に足して、足したボタンを選ぶ。
    fn add_button_copies(&mut self, menu: MenuId, clip: &[ClipButton]) {
        let existing: Vec<Rect> = self
            .session
            .project
            .menu(menu)
            .map(|menu| menu.buttons.iter().map(|button| button.rect).collect())
            .unwrap_or_default();
        let frame = frame_size(&self.session.project.settings);
        let Some(placed) = paste_positions(clip, &existing, frame) else {
            self.status = tl!("There is no room for the buttons on this menu").to_string();
            return;
        };
        let added = self.run_command(
            "menu.pasteButtons",
            crate::gui_params::paste_buttons(menu.0, &placed),
        );
        self.explain_overlap();
        let indexes: Vec<usize> = added
            .as_ref()
            .and_then(|value| value.get("buttons"))
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| {
                        value.as_u64().and_then(|index| usize::try_from(index).ok())
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !indexes.is_empty() {
            self.set_button_selection(menu, indexes);
        }
    }

    /// 編集 ▸ すべてを選択。
    pub(crate) fn select_all_buttons(&mut self) {
        let Some(menu) = self.edit_menu_target() else {
            return;
        };
        let count = self
            .session
            .project
            .menu(menu)
            .map_or(0, |menu| menu.buttons.len());
        if count > 0 {
            self.set_button_selection(menu, (0..count).collect());
        }
    }

    /// 編集 ▸ すべてを選択を解除。
    pub(crate) fn deselect_all_buttons(&mut self) {
        self.selected_buttons.clear();
    }

    /// 選んでいるボタンを消す（編集 ▸ 消去、切り取り、キャンバスの右クリック）。
    pub(crate) fn remove_selected_buttons(&mut self) {
        let Some(menu) = self.edit_menu_target() else {
            return;
        };
        if self.selected_buttons.is_empty() {
            return;
        }
        let indexes = std::mem::take(&mut self.selected_buttons);
        self.run_command(
            "menu.removeButtons",
            crate::gui_params::button_indexes(menu.0, &indexes),
        );
    }

    /// 動かしたボタンの位置を確定する（1 つなら `button.setRect`、いくつもなら `button.setRects`）。
    pub(crate) fn commit_button_rects(&mut self, menu: MenuId, rects: &[(usize, Rect)]) {
        match rects {
            [] => {}
            [(index, rect)] => self.set_button_rect(menu, *index, *rect),
            _ => {
                self.run_command(
                    "button.setRects",
                    crate::gui_params::set_button_rects(menu.0, rects),
                );
            }
        }
        self.explain_overlap();
    }

    /// ボタンが重なるために engine に断られたときは、検査の内部の言葉の代わりに分かる言葉で出す。
    fn explain_overlap(&mut self) {
        if self.status.contains("buttonsOverlap") {
            self.status = tl!("Buttons cannot overlap").to_string();
        }
    }

    /// メニュービューアで、選んでいるボタンを矢印キーで動かす（Shift で 10 ピクセルずつ）。
    pub(crate) fn nudge_buttons(&mut self, ctx: &egui::Context, menu: &Menu, frame: (u16, u16)) {
        if self.selected_buttons.is_empty() || ctx.egui_wants_keyboard_input() {
            return;
        }
        let (mut dx, mut dy) = (0_i32, 0_i32);
        ctx.input_mut(|input| {
            let step = if input.modifiers.shift { BIG_NUDGE } else { 1 };
            for (key, x, y) in [
                (egui::Key::ArrowLeft, -1, 0),
                (egui::Key::ArrowRight, 1, 0),
                (egui::Key::ArrowUp, 0, -1),
                (egui::Key::ArrowDown, 0, 1),
            ] {
                let modifiers = input.modifiers;
                if input.consume_key(modifiers, key) {
                    dx += x * step;
                    dy += y * step;
                }
            }
        });
        if dx == 0 && dy == 0 {
            return;
        }
        let rects: Vec<(usize, Rect)> = self
            .selected_buttons
            .iter()
            .filter_map(|index| menu.buttons.get(*index).map(|button| (*index, button.rect)))
            .collect();
        let moved = moved(&rects, dx, dy, frame);
        if moved != rects {
            self.commit_button_rects(menu.id, &moved);
        }
    }

    /// 整列と均等配置に要るだけボタンを選んでいるか（セーフエリアに相対的なら 1 つ、そうでなければ 2 つ）。
    pub(crate) fn can_arrange(&self, to_safe_area: bool) -> bool {
        let needed = if to_safe_area { 1 } else { 2 };
        self.edit_menu_target().is_some() && self.selected_buttons.len() >= needed
    }

    /// オブジェクト ▸ アレンジ。DiscCraft ではボタンの重ね順は番号なので、番号を並べ替える。
    pub(crate) fn arrange_selected(&mut self, arrange: Arrange) {
        let Some(menu) = self.edit_menu_target() else {
            return;
        };
        let count = self
            .session
            .project
            .menu(menu)
            .map_or(0, |menu| menu.buttons.len());
        if self.selected_buttons.is_empty() {
            return;
        }
        let order = arranged_order(count, &self.selected_buttons, arrange);
        self.apply_order(menu, &order);
    }

    /// ボタンのシートの「番号」: 主のボタンを `to` 番目（0 始まり）に移す。
    pub(crate) fn renumber_button(&mut self, menu: MenuId, from: usize, to: usize) {
        let count = self
            .session
            .project
            .menu(menu)
            .map_or(0, |menu| menu.buttons.len());
        self.apply_order(menu, &moved_order(count, from, to));
    }

    /// 番号を並べ替え、同じボタンを選んだままにする（番号は新しいものに読み替える）。
    fn apply_order(&mut self, menu: MenuId, order: &[usize]) {
        if order.iter().enumerate().all(|(new, old)| new == *old) {
            return;
        }
        let done = self.run_command(
            "menu.reorderButtons",
            crate::gui_params::reorder_buttons(menu.0, order),
        );
        if done.is_some() {
            let selection: Vec<usize> = self
                .selected_buttons
                .iter()
                .filter_map(|old| order.iter().position(|index| index == old))
                .collect();
            self.set_button_selection(menu, selection);
        }
    }

    /// オブジェクト ▸ 整列。
    pub(crate) fn align_selected(&mut self, align: Align) {
        let Some(menu) = self.edit_menu_target() else {
            return;
        };
        if !self.can_arrange(self.align_to_safe_area) {
            self.status = tl!("Select two or more buttons").to_string();
            return;
        }
        self.run_command(
            "menu.alignButtons",
            crate::gui_params::align_buttons(
                menu.0,
                &self.selected_buttons,
                align,
                self.align_to_safe_area,
            ),
        );
        self.explain_overlap();
    }

    /// オブジェクト ▸ 均等配置。
    pub(crate) fn distribute_selected(&mut self, axis: Axis) {
        let Some(menu) = self.edit_menu_target() else {
            return;
        };
        if !self.can_arrange(self.distribute_to_safe_area) {
            self.status = tl!("Select two or more buttons").to_string();
            return;
        }
        self.run_command(
            "menu.distributeButtons",
            crate::gui_params::distribute_buttons(
                menu.0,
                &self.selected_buttons,
                axis,
                self.distribute_to_safe_area,
            ),
        );
        self.explain_overlap();
    }
}

#[cfg(test)]
mod tests {
    use super::{ClipButton, clipboard_text, moved, parse_clipboard, paste_positions, toggled};
    use disccraft_project::{Link, Rect};

    fn rect(x: u16, y: u16, w: u16, h: u16) -> Rect {
        Rect { x, y, w, h }
    }

    fn clip(rect: Rect) -> ClipButton {
        ClipButton {
            label: "Play".to_string(),
            rect,
            target: Link::Stop,
            auto_activate: true,
        }
    }

    #[test]
    fn clipboard_round_trips_and_ignores_other_text() {
        let buttons = vec![clip(rect(10, 20, 100, 40))];
        let text = clipboard_text(&buttons).unwrap_or_default();
        assert_eq!(parse_clipboard(&text), Some(buttons));
        assert_eq!(parse_clipboard("hello"), None);
        assert_eq!(parse_clipboard(r#"{"disccraftButtons": []}"#), None);
    }

    #[test]
    fn paste_moves_off_the_buttons_it_would_cover() {
        let existing = [rect(160, 120, 400, 48), rect(160, 184, 400, 48)];
        let placed = paste_positions(&[clip(rect(160, 120, 400, 48))], &existing, (720, 480));
        let rects: Vec<Rect> = placed
            .unwrap_or_default()
            .iter()
            .map(|button| button.rect)
            .collect();
        // Right above the first button is the nearest room.
        assert_eq!(rects, vec![rect(160, 72, 400, 48)]);

        let free = paste_positions(&[clip(rect(300, 300, 100, 40))], &existing, (720, 480));
        assert_eq!(
            free.and_then(|buttons| buttons.first().map(|button| button.rect)),
            Some(rect(300, 300, 100, 40))
        );
    }

    #[test]
    fn a_group_stops_at_the_frame_edge_together() {
        let rects = [(0, rect(10, 100, 100, 40)), (2, rect(200, 100, 100, 40))];
        let left = moved(&rects, -50, 0, (720, 480));
        assert_eq!(
            left,
            vec![(0, rect(0, 100, 100, 40)), (2, rect(190, 100, 100, 40))]
        );
        let down = moved(&rects, 0, 1000, (720, 480));
        assert_eq!(
            down,
            vec![(0, rect(10, 440, 100, 40)), (2, rect(200, 440, 100, 40))]
        );
    }

    #[test]
    fn toggling_adds_and_removes() {
        assert_eq!(toggled(&[1, 3], 2), vec![1, 3, 2]);
        assert_eq!(toggled(&[1, 3], 1), vec![3]);
    }
}
