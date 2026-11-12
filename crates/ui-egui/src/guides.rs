//! メニュービューアのガイド（Encore の 表示 ▸ ガイドを表示 / ガイドにスナップ / ガイドをロック / ガイドを消去 /
//! 新規ガイド…）。ガイドはメニューに保存され（`Menu::guides`）、ボタンのドラッグがガイドに吸い付く。
//! 選択ツールでガイドをつかんで動かせ、絵の外へ出すと消える。

use crate::app::{DiscCraftApp, Selection};
use crate::i18n::{tl, tlf};
use crate::menu_editor::{canvas_scale, to_frame_point};

use disccraft_project::{Guide, GuideOrientation, Menu, MenuId, Rect};
use egui::{Context, Id, Modal, Ui};
use serde_json::json;

/// ガイドをつかめる距離（キャンバス上のピクセル）。
const GRAB_PX: f32 = 4.0;
/// ボタンがガイドに吸い付く距離（キャンバス上のピクセル）。
const SNAP_PX: f32 = 6.0;

/// 「新規ガイド」ダイアログの状態。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GuideDialog {
    /// 追加先のメニュー。
    menu: MenuId,
    /// 水平ガイドか（そうでなければ垂直）。
    horizontal: bool,
    /// 位置（ピクセル）の入力。
    position: String,
}

/// ドラッグ中のガイド。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GuideDrag {
    /// そのメニュー。
    pub(crate) menu: MenuId,
    /// `Menu::guides` の添字。
    pub(crate) index: usize,
    /// 今の位置（ディスク座標。離したときに確定する）。
    pub(crate) position: u16,
}

impl DiscCraftApp {
    /// ビューアに出ているメニュー（ガイドの操作の対象）。
    fn guide_menu(&self) -> Option<MenuId> {
        match self.selection {
            Selection::Menu(id) => Some(id),
            _ => None,
        }
    }

    /// 選択中のメニューにガイドがあるか（「ガイドを消去」が使えるか）。
    pub(crate) fn has_guides(&self) -> bool {
        self.guide_menu()
            .and_then(|id| self.session.project.menu(id))
            .is_some_and(|menu| !menu.guides.is_empty())
    }

    /// 「ガイドを消去」。
    pub(crate) fn clear_guides(&mut self) {
        if let Some(menu) = self.guide_menu()
            && self.has_guides()
        {
            let _ = self.run_command("menu.clearGuides", json!({"menu": menu.0}));
        }
    }

    /// 「新規ガイド…」のダイアログを開く。
    pub(crate) fn open_guide_dialog(&mut self) {
        if let Some(menu) = self.guide_menu() {
            self.guide_dialog = Some(GuideDialog {
                menu,
                horizontal: true,
                position: "0".to_owned(),
            });
        }
    }

    /// 「新規ガイド」ダイアログ（Encore と同じ: 方向のラジオボタン、位置のピクセル、OK / キャンセル）。
    pub(crate) fn guide_dialog_window(&mut self, ctx: &Context) {
        let Some(mut state) = self.guide_dialog.clone() else {
            return;
        };
        let frame = disccraft_project::frame_size(&self.session.project.settings);
        let limit = if state.horizontal { frame.1 } else { frame.0 };
        let position = state
            .position
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|position| *position <= limit);

        let mut done = None;
        let response = Modal::new(Id::unique("guide_dialog")).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.heading(tl!("New Guide"));
            ui.add_space(6.0);
            let _ = egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(tl!("Orientation"));
                    if ui
                        .radio(state.horizontal, tl!("Horizontal Guide"))
                        .clicked()
                    {
                        state.horizontal = true;
                    }
                    if ui.radio(!state.horizontal, tl!("Vertical Guide")).clicked() {
                        state.horizontal = false;
                    }
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(tl!("Position:"));
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut state.position)
                        .desired_width(60.0)
                        .align(egui::Align2::RIGHT_CENTER),
                );
                ui.label(tl!("pixels"));
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
            });
            if position.is_none() {
                ui.small(tlf!("0 to {max}", max = limit));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(position.is_some(), egui::Button::new(tl!("OK")))
                    .clicked()
                {
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
                self.guide_dialog = None;
                if let Some(position) = position {
                    let orientation = if state.horizontal {
                        "horizontal"
                    } else {
                        "vertical"
                    };
                    let _ = self.run_command(
                        "menu.addGuide",
                        json!({"menu": state.menu.0, "orientation": orientation, "position": position}),
                    );
                    self.show_guides = true;
                }
            }
            Some(false) => self.guide_dialog = None,
            None => self.guide_dialog = Some(state),
        }
    }

    /// ガイドの線をキャンバスいっぱいに描く（ドラッグ中のものは今の位置）。
    pub(crate) fn draw_guides(&self, ui: &Ui, menu: &Menu, frame: (u16, u16), canvas: egui::Rect) {
        if !self.show_guides {
            return;
        }
        let scale = canvas_scale(frame, canvas);
        let alpha = if self.lock_guides { 140 } else { 220 };
        let color = egui::Color32::from_rgba_unmultiplied(80, 190, 255, alpha);
        let stroke = egui::Stroke::new(1.0, color);
        for (index, guide) in menu.guides.iter().enumerate() {
            let position = match self.guide_drag {
                Some(drag) if drag.menu == menu.id && drag.index == index => drag.position,
                _ => guide.position,
            };
            match guide.orientation {
                GuideOrientation::Horizontal => {
                    let y = canvas.min.y + f32::from(position) * scale.y;
                    ui.painter().hline(canvas.x_range(), y, stroke);
                }
                GuideOrientation::Vertical => {
                    let x = canvas.min.x + f32::from(position) * scale.x;
                    ui.painter().vline(x, canvas.y_range(), stroke);
                }
            }
        }
    }

    /// その位置でつかめるガイド（表示中でロックされていないとき）。
    pub(crate) fn hit_guide(
        &self,
        menu: &Menu,
        frame: (u16, u16),
        canvas: egui::Rect,
        point: egui::Pos2,
    ) -> Option<(usize, Guide)> {
        if !self.show_guides || self.lock_guides || !canvas.contains(point) {
            return None;
        }
        let scale = canvas_scale(frame, canvas);
        menu.guides
            .iter()
            .enumerate()
            .filter_map(|(index, guide)| {
                let distance = match guide.orientation {
                    GuideOrientation::Horizontal => {
                        (canvas.min.y + f32::from(guide.position) * scale.y - point.y).abs()
                    }
                    GuideOrientation::Vertical => {
                        (canvas.min.x + f32::from(guide.position) * scale.x - point.x).abs()
                    }
                };
                (distance <= GRAB_PX).then_some((index, *guide, distance))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(index, guide, _)| (index, guide))
    }

    /// ガイドのドラッグ中: 位置を追いかけ、離したら確定する。絵の外で離すとそのガイドは消える。
    pub(crate) fn continue_guide_drag(
        &mut self,
        menu: &Menu,
        drag: GuideDrag,
        pointer: Option<egui::Pos2>,
        released: bool,
        frame: (u16, u16),
        canvas: egui::Rect,
    ) {
        if drag.menu != menu.id {
            self.guide_drag = None;
            return;
        }
        let Some(guide) = menu.guides.get(drag.index).copied() else {
            self.guide_drag = None;
            return;
        };
        let mut drag = drag;
        if let Some(point) = pointer {
            let (x, y) = to_frame_point(point, frame, canvas);
            let position = match guide.orientation {
                GuideOrientation::Horizontal => y,
                GuideOrientation::Vertical => x,
            };
            drag.position = u16::try_from(position.max(0)).unwrap_or(u16::MAX);
            if released {
                self.guide_drag = None;
                if !canvas.expand(GRAB_PX).contains(point) {
                    let _ = self.run_command(
                        "menu.removeGuide",
                        json!({"menu": menu.id.0, "index": drag.index}),
                    );
                } else if drag.position != guide.position {
                    let _ = self.run_command(
                        "menu.moveGuide",
                        json!({"menu": menu.id.0, "index": drag.index, "position": drag.position}),
                    );
                }
                return;
            }
        } else if released {
            self.guide_drag = None;
            return;
        }
        self.guide_drag = Some(drag);
    }

    /// ボタンをまとめて動かすときの移動量をガイドに吸い付ける（押したボタンの左右 / 上下の辺と中心で）。
    pub(crate) fn snap_move(
        &self,
        menu: &Menu,
        start: Rect,
        delta: (i32, i32),
        frame: (u16, u16),
        canvas: egui::Rect,
    ) -> (i32, i32) {
        if !self.snaps_to_guides(menu) {
            return delta;
        }
        let threshold = snap_threshold(frame, canvas);
        let x = i32::from(start.x).saturating_add(delta.0);
        let y = i32::from(start.y).saturating_add(delta.1);
        let w = i32::from(start.w);
        let h = i32::from(start.h);
        let dx = snap_delta(
            &[x, x.saturating_add(w), x.saturating_add(w / 2)],
            &guide_positions(menu, GuideOrientation::Vertical),
            threshold.0,
        );
        let dy = snap_delta(
            &[y, y.saturating_add(h), y.saturating_add(h / 2)],
            &guide_positions(menu, GuideOrientation::Horizontal),
            threshold.1,
        );
        (
            delta.0.saturating_add(dx.unwrap_or(0)),
            delta.1.saturating_add(dy.unwrap_or(0)),
        )
    }

    /// ボタンの大きさを変えるときの右辺と下辺をガイドに吸い付ける。
    pub(crate) fn snap_resize(
        &self,
        menu: &Menu,
        rect: Rect,
        frame: (u16, u16),
        canvas: egui::Rect,
    ) -> Rect {
        if !self.snaps_to_guides(menu) {
            return rect;
        }
        let threshold = snap_threshold(frame, canvas);
        let right = i32::from(rect.x).saturating_add(i32::from(rect.w));
        let bottom = i32::from(rect.y).saturating_add(i32::from(rect.h));
        let dw = snap_delta(
            &[right],
            &guide_positions(menu, GuideOrientation::Vertical),
            threshold.0,
        )
        .unwrap_or(0);
        let dh = snap_delta(
            &[bottom],
            &guide_positions(menu, GuideOrientation::Horizontal),
            threshold.1,
        )
        .unwrap_or(0);
        let min = i32::from(crate::menu_editor::MIN_SIDE);
        Rect {
            x: rect.x,
            y: rect.y,
            w: u16::try_from(i32::from(rect.w).saturating_add(dw).max(min)).unwrap_or(rect.w),
            h: u16::try_from(i32::from(rect.h).saturating_add(dh).max(min)).unwrap_or(rect.h),
        }
    }

    /// 吸い付きが効くか: 表示中のガイドがあり、「ガイドにスナップ」が入っているとき。
    fn snaps_to_guides(&self, menu: &Menu) -> bool {
        self.snap_to_guides && self.show_guides && !menu.guides.is_empty()
    }
}

/// 吸い付く距離をディスク座標で（横、縦）。
fn snap_threshold(frame: (u16, u16), canvas: egui::Rect) -> (i32, i32) {
    let scale = canvas_scale(frame, canvas);
    let to_frame = |scale: f32| -> i32 {
        if scale > 0.0 && scale.is_finite() {
            // Rounded up so one canvas pixel still snaps on a small canvas; `as` saturates.
            (SNAP_PX / scale).ceil().clamp(1.0, 64.0) as i32
        } else {
            1
        }
    };
    (to_frame(scale.x), to_frame(scale.y))
}

/// そのメニューのガイドの位置（向きで選ぶ）。
fn guide_positions(menu: &Menu, orientation: GuideOrientation) -> Vec<i32> {
    menu.guides
        .iter()
        .filter(|guide| guide.orientation == orientation)
        .map(|guide| i32::from(guide.position))
        .collect()
}

/// `edges` のどれかを `threshold` 以内のガイドへ動かす最小の移動量。届くガイドがなければ `None`。
fn snap_delta(edges: &[i32], guides: &[i32], threshold: i32) -> Option<i32> {
    edges
        .iter()
        .flat_map(|edge| guides.iter().map(move |guide| guide.saturating_sub(*edge)))
        .filter(|delta| delta.abs() <= threshold)
        .min_by_key(|delta| delta.abs())
}

#[cfg(test)]
mod tests {
    use super::{snap_delta, snap_threshold};

    #[test]
    fn the_nearest_guide_within_reach_wins() {
        assert_eq!(snap_delta(&[100, 200], &[104, 196], 6), Some(4));
        assert_eq!(snap_delta(&[100, 200], &[104, 197], 6), Some(-3));
        assert_eq!(snap_delta(&[100], &[110], 6), None);
        assert_eq!(snap_delta(&[100], &[], 6), None);
        assert_eq!(snap_delta(&[], &[100], 6), None);
    }

    #[test]
    fn the_snap_distance_follows_the_zoom() {
        let frame = (720, 480);
        // Shown at half size, 6 canvas pixels are 12 frame pixels.
        let half = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(360.0, 240.0));
        assert_eq!(snap_threshold(frame, half), (12, 12));
        // Shown at double size, 3 frame pixels.
        let double = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 960.0));
        assert_eq!(snap_threshold(frame, double), (3, 3));
        let empty = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::ZERO);
        assert_eq!(snap_threshold(frame, empty), (1, 1));
    }
}
