//! タイムライン ▸ 字幕の読み込み ▸ FAB 画像スクリプト… / 画像スクリプト…（Encore と同じ）: スクリプトを選び（FAB は
//! 絵のフォルダーも）、1 枚目の絵の上でスポイトを使って 背景 / 塗り（カラー 1）/ 輪郭（カラー 2）の色を決め
//! （Encore の「カラーをマップ」）、タイムコードを 絶対 / 相対 から選んで、字幕トラックの追加ダイアログへ進む。
//! 読み込みは engine の `subtitle.readImageScript` / `subtitle.importImageScript`。

use std::path::{Path, PathBuf};

use crate::DiscCraftApp;
use crate::i18n::{tl, tlf};
use crate::track_dialog::TrackKind;

use crate::file_dialogs::{Then, ask};
use craft_dialogs::FileDialog;
use disccraft_project::{AssetId, ImageCueColors, TimelineId};
use serde_json::{Map, Value, json};

/// The largest the picture is shown in the dialog, points.
const SHOWN: egui::Vec2 = egui::vec2(360.0, 240.0);

/// Which colour a click on the picture picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    Background,
    Fill,
    Outline,
}

/// The first picture of the script: its pixels (to pick from) and as shown.
pub(crate) struct Picture {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
    texture: egui::TextureHandle,
}

/// An image script being imported: what was read of it, and the choices of the dialog.
pub(crate) struct ImageScriptImport {
    timeline: TimelineId,
    path: PathBuf,
    fab: bool,
    folder: Option<PathBuf>,
    cues: u64,
    missing: Vec<String>,
    missing_count: u64,
    picture: Option<Picture>,
    colors: ImageCueColors,
    pick: Pick,
    relative: bool,
    offset: String,
    /// The picture as the subtitle will be drawn, and the colours it was made with.
    preview: Option<(ImageCueColors, egui::TextureHandle)>,
}

impl ImageScriptImport {
    fn color_mut(&mut self, pick: Pick) -> &mut [u8; 3] {
        match pick {
            Pick::Background => &mut self.colors.background,
            Pick::Fill => &mut self.colors.fill,
            Pick::Outline => &mut self.colors.outline,
        }
    }
}

/// `#rrggbb`.
fn hex(color: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}

/// A `#rrggbb` colour.
fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// A Relative offset typed as a timecode (`-` in front moves the subtitles earlier), in ticks.
fn parse_offset(text: &str, settings: &disccraft_project::DiscSettings) -> Option<i64> {
    let text = text.trim();
    let (sign, timecode) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest.trim()),
        None => (1, text),
    };
    disccraft_project::parse_time(timecode, settings)
        .ok()
        .map(|tick| tick.0.saturating_mul(sign))
}

/// Where a click at `at` over a picture shown in `rect` falls in its pixels.
fn pixel_at(
    rect: egui::Rect,
    at: egui::Pos2,
    (width, height): (usize, usize),
) -> Option<(usize, usize)> {
    if !rect.contains(at) || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return None;
    }
    // `as` saturates; the position is inside the picture.
    let x = ((at.x - rect.min.x) / rect.width() * width as f32) as usize;
    let y = ((at.y - rect.min.y) / rect.height() * height as f32) as usize;
    Some((
        x.min(width.saturating_sub(1)),
        y.min(height.saturating_sub(1)),
    ))
}

impl DiscCraftApp {
    /// タイムライン ▸ 字幕の読み込み ▸ FAB 画像スクリプト… (`fab`) / 画像スクリプト…: スクリプト（FAB は絵の
    /// フォルダーも）を選んで読み、カラーをマップのダイアログを開く。
    pub(crate) fn import_image_script(&mut self, fab: bool) {
        let Some(timeline) = self.selected_timeline() else {
            self.status = tl!("Select a timeline first").to_string();
            return;
        };
        let title = if fab {
            tl!("FAB Image Script")
        } else {
            tl!("Image Script")
        };
        ask(
            FileDialog::open().title(title).filter(title, &["txt"]),
            Then::ImageScript { timeline, fab },
        );
    }

    /// The image script chosen: a FAB script names no folder, so (as Encore does) the folder its pictures are in is
    /// asked for next; another is read now.
    pub(crate) fn image_script_chosen(
        &mut self,
        ctx: &egui::Context,
        timeline: TimelineId,
        fab: bool,
        path: PathBuf,
    ) {
        if fab {
            let mut dialog = FileDialog::folder().title(tl!("Folder of the Pictures"));
            if let Some(start) = path.parent() {
                dialog = dialog.directory(start);
            }
            ask(
                dialog,
                Then::ImageScriptFolder {
                    timeline,
                    script: path,
                },
            );
            return;
        }
        self.read_image_script(ctx, timeline, path, None);
    }

    /// Reads the image script `path` (a FAB script with the `folder` of its pictures) and opens the Map Colors
    /// dialog.
    pub(crate) fn read_image_script(
        &mut self,
        ctx: &egui::Context,
        timeline: TimelineId,
        path: PathBuf,
        folder: Option<PathBuf>,
    ) {
        let fab = folder.is_some();
        let mut params = Map::new();
        params.insert("path".to_owned(), json!(path.to_string_lossy()));
        params.insert("fab".to_owned(), json!(fab));
        if let Some(folder) = &folder {
            params.insert("folder".to_owned(), json!(folder.to_string_lossy()));
        }
        let Some(read) = self.run_query("subtitle.readImageScript", Value::Object(params)) else {
            return;
        };
        let color = |key: &str, fallback: [u8; 3]| {
            read.pointer(&format!("/colors/{key}"))
                .and_then(Value::as_str)
                .and_then(parse_hex)
                .unwrap_or(fallback)
        };
        let colors = ImageCueColors {
            background: color("background", [0, 0, 255]),
            fill: color("fill", [255, 255, 255]),
            outline: color("outline", [0, 0, 0]),
        };
        let picture = read
            .get("first")
            .and_then(Value::as_str)
            .and_then(|first| disccraft_menu::subtitle::load_picture(Path::new(first)).ok())
            .and_then(|(width, height, rgba)| {
                let (width, height) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
                let shown = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
                let texture =
                    ctx.load_texture("image-script-picture", shown, egui::TextureOptions::NEAREST);
                Some(Picture {
                    width,
                    height,
                    rgba,
                    texture,
                })
            });
        let count = |key: &str| read.get(key).and_then(Value::as_u64).unwrap_or(0);
        self.image_script_import = Some(ImageScriptImport {
            timeline,
            path,
            fab,
            folder,
            cues: count("cues"),
            missing: read
                .get("missing")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            missing_count: count("missingCount"),
            picture,
            colors,
            pick: Pick::Fill,
            relative: false,
            offset: "00;00;00;00".to_owned(),
            preview: None,
        });
    }

    /// カラーをマップのダイアログ（画像スクリプトの読み込み中だけ）。OK で読み込み、字幕トラックの追加へ進む。
    pub(crate) fn image_script_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.image_script_import.take() else {
            return;
        };
        let group = self
            .session
            .project
            .timeline_subtitle_colors(dialog.timeline)
            .group(0);
        let (fill, stroke) = (
            crate::monitor::subtitle_color(group.fill),
            crate::monitor::subtitle_color(group.stroke),
        );
        // The picture as the subtitle will be drawn, made again when a colour changes.
        if dialog.preview.as_ref().map(|(colors, _)| *colors) != Some(dialog.colors)
            && let Some(picture) = &dialog.picture
        {
            let pixels = picture
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| {
                    match disccraft_menu::subtitle::image_pixel_value(pixel, &dialog.colors) {
                        1 => fill,
                        2 => stroke,
                        _ => egui::Color32::TRANSPARENT,
                    }
                })
                .collect();
            let image = egui::ColorImage::new([picture.width, picture.height], pixels);
            let texture =
                ctx.load_texture("image-script-preview", image, egui::TextureOptions::NEAREST);
            dialog.preview = Some((dialog.colors, texture));
        }
        let offset = parse_offset(&dialog.offset, &self.session.project.settings);
        let mut open = true;
        let mut finish = None;
        egui::Window::new(tl!("Map Colors"))
            .id(egui::Id::unique("image_script_map_colors"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let name = dialog
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                ui.label(tlf!("{file}: {n} subtitles", file = name, n = dialog.cues));
                if dialog.missing_count > 0 {
                    crate::theme::status_label(
                        ui,
                        crate::theme::Status::Error,
                        tlf!(
                            "{n} pictures are not found, such as {path}",
                            n = dialog.missing_count,
                            path = dialog.missing.first().cloned().unwrap_or_default()
                        ),
                    );
                }
                ui.label(tl!(
                    "Click the picture to pick the colour of the chosen item."
                ));
                let mut picked = None;
                if let Some(picture) = &dialog.picture {
                    let scale = (SHOWN.x / picture.width.max(1) as f32)
                        .min(SHOWN.y / picture.height.max(1) as f32)
                        .min(1.0);
                    let size =
                        egui::vec2(picture.width as f32 * scale, picture.height as f32 * scale);
                    let full = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    picked = ui
                        .horizontal(|ui| {
                            let (rect, response) =
                                ui.allocate_exact_size(size, egui::Sense::click());
                            ui.painter().image(
                                picture.texture.id(),
                                rect,
                                full,
                                egui::Color32::WHITE,
                            );
                            let picked = response
                                .clicked()
                                .then(|| response.interact_pointer_pos())
                                .flatten()
                                .and_then(|at| pixel_at(rect, at, (picture.width, picture.height)))
                                .and_then(|(x, y)| {
                                    let index = (y * picture.width + x) * 4;
                                    let pixel = picture.rgba.get(index..index + 3)?;
                                    Some([*pixel.first()?, *pixel.get(1)?, *pixel.get(2)?])
                                });
                            let _ = response.on_hover_cursor(egui::CursorIcon::Crosshair);
                            // As the subtitle will be drawn, over a grey ground.
                            let (shown, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                            ui.painter()
                                .rect_filled(shown, 0.0, egui::Color32::from_gray(90));
                            if let Some((_, preview)) = &dialog.preview {
                                ui.painter()
                                    .image(preview.id(), shown, full, egui::Color32::WHITE);
                            }
                            picked
                        })
                        .inner;
                }
                if let Some(color) = picked {
                    let pick = dialog.pick;
                    *dialog.color_mut(pick) = color;
                }
                ui.add_space(4.0);
                for (pick, label, meaning) in [
                    (Pick::Background, tl!("Background"), tl!("Transparent")),
                    (Pick::Fill, tl!("Fill / Color 1"), tl!("The group's fill")),
                    (
                        Pick::Outline,
                        tl!("Outline / Color 2"),
                        tl!("The group's stroke"),
                    ),
                ] {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut dialog.pick, pick, label);
                        let color = dialog.color_mut(pick);
                        ui.color_edit_button_srgb(color);
                        ui.monospace(hex(*color));
                        ui.weak(meaning);
                    });
                }
                ui.weak(tl!("Any other colour in the pictures is transparent."));
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(tl!("Timecode:"));
                    ui.radio_value(&mut dialog.relative, false, tl!("Absolute"));
                    ui.radio_value(&mut dialog.relative, true, tl!("Relative"));
                    ui.add_enabled(
                        dialog.relative,
                        egui::TextEdit::singleline(&mut dialog.offset).desired_width(110.0),
                    );
                });
                if dialog.relative && offset.is_none() {
                    crate::theme::status_label(
                        ui,
                        crate::theme::Status::Error,
                        tl!("The offset is not a timecode."),
                    );
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let ready = dialog.missing_count == 0
                        && dialog.cues > 0
                        && (!dialog.relative || offset.is_some());
                    if ui
                        .add_enabled(ready, crate::theme::default_button(tl!("OK")))
                        .clicked()
                    {
                        finish = Some(true);
                    }
                    if ui.add(crate::theme::dialog_button(tl!("Cancel"))).clicked() {
                        finish = Some(false);
                    }
                });
            });
        match finish {
            Some(true) => {
                let offset = if dialog.relative {
                    offset.unwrap_or(0)
                } else {
                    0
                };
                self.finish_image_script(&dialog, offset);
            }
            Some(false) => {}
            None if open => self.image_script_import = Some(dialog),
            None => {}
        }
    }

    /// 画像スクリプトを字幕アセットにして、字幕トラックの追加ダイアログを開く。
    fn finish_image_script(&mut self, dialog: &ImageScriptImport, offset: i64) {
        let mut params = Map::new();
        params.insert("path".to_owned(), json!(dialog.path.to_string_lossy()));
        params.insert("fab".to_owned(), json!(dialog.fab));
        if let Some(folder) = &dialog.folder {
            params.insert("folder".to_owned(), json!(folder.to_string_lossy()));
        }
        params.insert(
            "background".to_owned(),
            json!(hex(dialog.colors.background)),
        );
        params.insert("fill".to_owned(), json!(hex(dialog.colors.fill)));
        params.insert("outline".to_owned(), json!(hex(dialog.colors.outline)));
        if offset != 0 {
            params.insert("offset".to_owned(), json!(offset));
        }
        let Some(imported) = self.run_command("subtitle.importImageScript", Value::Object(params))
        else {
            return;
        };
        let Some(asset) = imported.get("asset").and_then(Value::as_u64).map(AssetId) else {
            return;
        };
        let count = imported.get("cues").and_then(Value::as_u64).unwrap_or(0);
        self.subtitle_cache.remove(&asset.0);
        let language =
            crate::subtitles::language_from_name(&dialog.path).unwrap_or_else(|| "ja".to_owned());
        self.open_track_dialog_with(dialog.timeline, TrackKind::Subtitle, asset, language);
        self.status = tlf!("Read {n} subtitles", n = count);
    }
}

#[cfg(test)]
mod tests {
    use super::{hex, parse_hex, parse_offset, pixel_at};

    #[test]
    fn colours_offsets_and_clicks_are_read() {
        assert_eq!(parse_hex("#1eb43c"), Some([0x1e, 0xb4, 0x3c]));
        assert_eq!(hex([0x1e, 0xb4, 0x3c]), "#1eb43c");
        assert_eq!(parse_hex("1eb43c"), None);
        assert_eq!(parse_hex("#1eb43"), None);
        let settings = disccraft_project::DiscSettings {
            standard: disccraft_project::VideoStandard::Pal,
            ..disccraft_project::DiscSettings::default()
        };
        let second = 254_016_000_000;
        assert_eq!(parse_offset("00:00:01:00", &settings), Some(second));
        assert_eq!(parse_offset("-00:00:01:00", &settings), Some(-second));
        assert_eq!(parse_offset("soon", &settings), None);
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(100.0, 50.0));
        assert_eq!(
            pixel_at(rect, egui::pos2(10.0, 10.0), (200, 100)),
            Some((0, 0))
        );
        assert_eq!(
            pixel_at(rect, egui::pos2(109.9, 59.9), (200, 100)),
            Some((199, 99))
        );
        assert_eq!(pixel_at(rect, egui::pos2(5.0, 10.0), (200, 100)), None);
    }
}
