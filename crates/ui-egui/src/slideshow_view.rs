//! スライドショービューア（Encore と同じ形。[docs/ui-parity.md](../../../docs/ui-parity.md) §3.5）。
//!
//! 左の「スライド」にスライドをタイルで並べ、右の「スライドショーオプション」でオーディオとスライドの
//! デュレーション、スライドの追加を扱う。下端に全体のデュレーションを出す。

use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf};
use crate::timeline_view::ticks_to_seconds;

use disccraft_project::{AssetId, AssetKind, TransitionKind, format_time};
use egui::{Rect, Sense, Ui, vec2};

/// スライドのタイルの大きさ。
const TILE: egui::Vec2 = egui::Vec2::new(132.0, 92.0);
/// 右のオプションの幅。
const OPTIONS_WIDTH: f32 = 290.0;

impl DiscCraftApp {
    /// スライドショービューア。
    pub(crate) fn slideshow_view(&mut self, ui: &mut Ui, id: u64) {
        let Some(slideshow) = self.session.project.slideshow(id).cloned() else {
            ui.label(tl!("Slideshow not found"));
            return;
        };

        let total = slideshow
            .slides
            .iter()
            .fold(0_i64, |sum, slide| sum.saturating_add(slide.duration.0));

        ui.horizontal_top(|ui| {
            let slides_width = (ui.available_width() - OPTIONS_WIDTH - 12.0).max(TILE.x + 16.0);
            ui.allocate_ui(vec2(slides_width, ui.available_height()), |ui| {
                ui.vertical(|ui| {
                    ui.strong(tl!("Slides"));
                    self.slide_tiles(ui, id, &slideshow.slides);
                });
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(OPTIONS_WIDTH);
                self.slideshow_options(ui, id, slideshow.audio);
            });
        });

        ui.separator();
        ui.label(tlf!(
            "Total duration: {time}",
            time = self.duration_text(ticks_to_seconds(total))
        ));
    }

    /// スライドのタイル（番号、絵、Encore と同じくインポイント）。押すと選び、プロパティがスライドのシートになる。
    /// 右クリックの 消去 か Delete で外す。
    fn slide_tiles(&mut self, ui: &mut Ui, id: u64, slides: &[disccraft_project::Slide]) {
        if slides.is_empty() {
            ui.label(tl!("Add images from the options on the right."));
            return;
        }

        let settings = self.session.project.settings.clone();
        let mut start = disccraft_project::Tick::ZERO;
        let tiles: Vec<(String, String, Option<egui::TextureHandle>)> = slides
            .iter()
            .map(|slide| {
                let tile = (
                    self.asset_name(slide.asset),
                    format_time(start, &settings),
                    self.asset_thumbnail(ui.ctx(), slide.asset),
                );
                start = disccraft_project::Tick(start.0.saturating_add(slide.duration.0));
                tile
            })
            .collect();

        let focused = self
            .slide_focus
            .filter(|(show, _)| *show == id)
            .map(|(_, index)| index);
        let mut remove = None;
        let mut focus = None;
        ui.horizontal_wrapped(|ui| {
            for (index, (name, in_point, texture)) in tiles.iter().enumerate() {
                let response = slide_tile(
                    ui,
                    index,
                    name,
                    in_point,
                    texture.as_ref(),
                    focused == Some(index),
                );
                if response.clicked() || response.secondary_clicked() {
                    focus = Some(index);
                }
                let _ = response.context_menu(|ui| {
                    if ui.button(tl!("Clear")).clicked() {
                        remove = Some(index);
                        ui.close();
                    }
                });
            }
        });
        if let Some(index) = focus {
            self.slide_focus = Some((id, index));
        }
        // Delete で選んだスライドを外す（入力欄にフォーカスがないとき）。
        if let Some(index) = focused
            && !ui.ctx().egui_wants_keyboard_input()
            && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        {
            remove = Some(index);
        }

        if let Some(index) = remove {
            self.remove_slide(id, index);
            self.slide_focus = None;
        }
    }

    /// 右のスライドショーオプション。
    fn slideshow_options(&mut self, ui: &mut Ui, id: u64, audio: Option<AssetId>) {
        ui.strong(tl!("Slideshow Options"));
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(tl!("Audio"));
            let _ = audio;
            self.slideshow_audio_rows(ui, id);
        });
        ui.add_space(4.0);
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            self.slide_duration_row(ui, id, "options");
            ui.horizontal(|ui| {
                ui.label(tl!("Default transition:"));
                self.transition_combo(ui, id, "options");
            });
            self.manual_advance_row(ui, id);
            self.add_slide_row(ui, id);
        });
    }

    /// 「手動で進める」のチェック（Encore と同じ。DVD では各スライドが次へ / 決定まで止まる）。
    pub(crate) fn manual_advance_row(&mut self, ui: &mut Ui, id: u64) {
        let Some(current) = self
            .session
            .project
            .slideshow(id)
            .map(|slideshow| slideshow.manual_advance)
        else {
            return;
        };
        let mut manual = current;
        if ui
            .checkbox(&mut manual, tl!("Manual advance"))
            .on_hover_text(tl!(
                "Each slide holds until the viewer presses Next or Enter (DVD)"
            ))
            .changed()
        {
            let _ = self.run_command(
                "slideshow.setManualAdvance",
                serde_json::json!({ "slideshow": id, "manual": manual }),
            );
        }
    }

    /// トランジションの種類のコンボ（Encore の「トランジションの初期設定」。`slideshow.setTransition`）。
    pub(crate) fn transition_combo(&mut self, ui: &mut Ui, id: u64, salt: &str) {
        let Some(current) = self
            .session
            .project
            .slideshow(id)
            .map(|slideshow| slideshow.transition)
        else {
            return;
        };
        let mut chosen = None;
        egui::ComboBox::from_id_salt(("slideshow_transition", salt, id))
            .selected_text(transition_label(current))
            .width(150.0)
            .show_ui(ui, |ui| {
                for kind in TransitionKind::ALL {
                    if ui
                        .selectable_label(current == kind, transition_label(kind))
                        .clicked()
                    {
                        chosen = Some(kind);
                    }
                }
            });
        if let Some(kind) = chosen.filter(|kind| *kind != current) {
            let _ = self.run_command(
                "slideshow.setTransition",
                serde_json::json!({ "slideshow": id, "kind": kind.as_str() }),
            );
        }
    }

    /// トランジションの長さのタイムコードの欄。
    pub(crate) fn transition_duration_row(&mut self, ui: &mut Ui, id: u64, salt: &str) {
        let Some(current) = self.session.project.slideshow(id).map(|slideshow| {
            format_time(
                slideshow.transition_duration,
                &self.session.project.settings,
            )
        }) else {
            return;
        };
        let typed = crate::properties::committed_text(
            ui,
            &format!("slideshow_transition_duration_{salt}_{id}"),
            &current,
            false,
        )
        .1;
        if let Some(text) = typed {
            let _ = self.run_command(
                "slideshow.setTransition",
                serde_json::json!({ "slideshow": id, "timecode": text.trim() }),
            );
        }
    }

    /// 「スライドのデュレーション」のタイムコードの欄（Encore と同じ。変えると、同じ長さだったスライドも変わる）。
    pub(crate) fn slide_duration_row(&mut self, ui: &mut Ui, id: u64, salt: &str) {
        let Some(current) =
            self.session.project.slideshow(id).map(|slideshow| {
                format_time(slideshow.slide_duration, &self.session.project.settings)
            })
        else {
            return;
        };
        let mut typed = None;
        ui.horizontal(|ui| {
            ui.label(tl!("Slide duration:"));
            // フォーカスが外れたとき（Enter でも）に確定する。
            typed = crate::properties::committed_text(
                ui,
                &format!("slideshow_slide_duration_{salt}_{id}"),
                &current,
                false,
            )
            .1;
        });
        if let Some(text) = typed {
            let _ = self.run_command(
                "slideshow.setDefaultDuration",
                serde_json::json!({ "slideshow": id, "timecode": text.trim() }),
            );
        }
    }

    /// オーディオの一覧（Encore と同じ 番号 / 名前 / デュレーション）、追加のコンボ、合わせる / ループのチェック。
    fn slideshow_audio_rows(&mut self, ui: &mut Ui, id: u64) {
        let Some(slideshow) = self.session.project.slideshow(id).cloned() else {
            return;
        };
        let clips = slideshow.audio_list();
        let mut remove = None;
        egui::Grid::new(("slideshow_audio_list", id))
            .num_columns(4)
            .spacing([8.0, 2.0])
            .show(ui, |ui| {
                ui.small(tl!("Number"));
                ui.small(tl!("Name"));
                ui.small(tl!("Duration"));
                ui.label("");
                ui.end_row();
                for (index, asset) in clips.iter().enumerate() {
                    ui.label(index.saturating_add(1).to_string());
                    ui.label(self.asset_name(*asset));
                    let probe = self.asset_probe(*asset);
                    match probe.seconds {
                        Some(seconds) => ui.label(self.duration_text_at(seconds, probe.fps)),
                        None => ui.label(""),
                    };
                    if ui.small_button("×").on_hover_text(tl!("Remove")).clicked() {
                        remove = Some(index);
                    }
                    ui.end_row();
                }
            });
        if let Some(index) = remove {
            let _ = self.run_command(
                "slideshow.removeAudioClip",
                serde_json::json!({ "slideshow": id, "index": index }),
            );
        }
        // 追加: オーディオアセットを選ぶと末尾に足す。
        let assets = self.assets_of(AssetKind::Audio);
        let mut chosen = None;
        egui::ComboBox::from_id_salt(("slideshow_audio_add", id))
            .selected_text(tl!("Add audio…"))
            .width(200.0)
            .show_ui(ui, |ui| {
                for (asset, name) in &assets {
                    if ui.selectable_label(false, name).clicked() {
                        chosen = Some(*asset);
                    }
                }
            });
        if let Some(asset) = chosen {
            let _ = self.run_command(
                "slideshow.addAudioClip",
                serde_json::json!({ "slideshow": id, "asset": asset.0 }),
            );
        }
        let mut fit = slideshow.fit_to_audio;
        if ui
            .checkbox(&mut fit, tl!("Fit Slideshow to Audio Duration"))
            .changed()
        {
            let _ = self.run_command(
                "slideshow.setAudioOptions",
                serde_json::json!({ "slideshow": id, "fit": fit }),
            );
        }
        let mut looped = slideshow.loop_audio;
        if ui.checkbox(&mut looped, tl!("Loop Audio")).changed() {
            let _ = self.run_command(
                "slideshow.setAudioOptions",
                serde_json::json!({ "slideshow": id, "loop": looped }),
            );
        }
    }

    /// オーディオの選択（なし / オーディオアセット）。`slideshow.setAudio` を送る。
    #[allow(dead_code)]
    fn slideshow_audio_combo(&mut self, ui: &mut Ui, id: u64, audio: Option<AssetId>) {
        let assets = self.assets_of(AssetKind::Audio);
        let current = audio
            .and_then(|asset| assets.iter().find(|(id, _)| *id == asset))
            .map_or_else(|| tl!("None").to_owned(), |(_, name)| name.clone());

        let mut chosen: Option<Option<AssetId>> = None;
        egui::ComboBox::from_id_salt(("slideshow_audio", id))
            .selected_text(current)
            .show_ui(ui, |ui| {
                if ui.selectable_label(audio.is_none(), tl!("None")).clicked() {
                    chosen = Some(None);
                }
                for (asset, name) in &assets {
                    if ui.selectable_label(audio == Some(*asset), name).clicked() {
                        chosen = Some(Some(*asset));
                    }
                }
            });

        if let Some(asset) = chosen {
            let value = asset.map_or(serde_json::Value::Null, |asset| {
                serde_json::Value::from(asset.0)
            });
            let _ = self.run_command(
                "slideshow.setAudio",
                serde_json::json!({ "slideshow": id, "asset": value }),
            );
        }
    }

    /// 画像を選んでスライドを足す行。
    fn add_slide_row(&mut self, ui: &mut Ui, id: u64) {
        let images = self.assets_of(AssetKind::Image);
        let current = self
            .slide_asset
            .and_then(|asset| images.iter().find(|(id, _)| *id == asset))
            .map_or_else(|| tl!("Image asset").to_owned(), |(_, name)| name.clone());

        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(("slide_asset", id))
                .selected_text(current)
                .width(150.0)
                .show_ui(ui, |ui| {
                    for (asset, name) in &images {
                        if ui
                            .selectable_label(self.slide_asset == Some(*asset), name)
                            .clicked()
                        {
                            self.slide_asset = Some(*asset);
                        }
                    }
                });
            let add = ui.add_enabled(
                self.slide_asset.is_some(),
                egui::Button::new(tl!("Add Slide")),
            );
            if add.clicked()
                && let Some(asset) = self.slide_asset
            {
                self.add_slide(id, asset);
            }
        });
    }

    /// 種類が `kind` のアセット（ID と名前）。
    pub(crate) fn assets_of(&self, kind: AssetKind) -> Vec<(AssetId, String)> {
        self.session
            .project
            .assets
            .iter()
            .filter(|(_, asset)| asset.kind == kind)
            .map(|(id, asset)| (*id, asset.name.clone()))
            .collect()
    }
}

/// トランジションの名前（Encore の一覧の名前）。
pub(crate) fn transition_label(kind: TransitionKind) -> &'static str {
    match kind {
        TransitionKind::None => tl!("None"),
        TransitionKind::CrossDissolve => tl!("Cross Dissolve"),
        TransitionKind::DipToBlack => tl!("Dip to Black"),
        TransitionKind::Wipe => tl!("Wipe"),
    }
}

/// 1 枚のスライドのタイル（Encore と同じく番号、絵、下にインポイント。選んでいるものは明るい枠）。
fn slide_tile(
    ui: &mut Ui,
    index: usize,
    name: &str,
    in_point: &str,
    texture: Option<&egui::TextureHandle>,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(TILE, Sense::click());
    let visuals = ui.visuals();
    let painter = ui.painter();
    let picture = Rect::from_min_size(rect.min, vec2(TILE.x, TILE.y - 34.0));
    painter.rect_filled(picture, 2.0, visuals.extreme_bg_color);
    if let Some(texture) = texture {
        crate::project_panel::draw_fitted(painter, picture.shrink(1.0), texture);
    }
    let frame_stroke = if selected {
        egui::Stroke::new(2.0, visuals.selection.stroke.color)
    } else if response.hovered() {
        visuals.widgets.hovered.bg_stroke
    } else {
        visuals.widgets.noninteractive.bg_stroke
    };
    painter.rect_stroke(rect, 2.0, frame_stroke, egui::StrokeKind::Inside);
    painter.rect_stroke(
        picture,
        2.0,
        visuals.widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    painter.text(
        picture.left_top() + vec2(4.0, 2.0),
        egui::Align2::LEFT_TOP,
        index.saturating_add(1).to_string(),
        egui::FontId::proportional(11.0),
        visuals.weak_text_color(),
    );
    if texture.is_none() {
        painter.text(
            picture.center(),
            egui::Align2::CENTER_CENTER,
            name,
            egui::FontId::proportional(11.0),
            visuals.text_color(),
        );
    }
    painter.text(
        egui::pos2(rect.center().x, picture.max.y + 4.0),
        egui::Align2::CENTER_TOP,
        in_point,
        egui::FontId::proportional(11.0),
        visuals.text_color(),
    );
    response
}
