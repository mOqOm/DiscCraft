//! 字幕ファイル: タイムライン ▸ 字幕を読み込み… / 字幕を書き出し、タイムラインの字幕トラックの字幕の表示、
//! モニターに今の字幕を重ねる。読み込みは engine の `subtitle.cues`（SRT、WebVTT、タイムコード付きテキスト。
//! UTF-8 / UTF-16 / Shift_JIS）。

use std::path::Path;
use std::sync::Arc;

use crate::DiscCraftApp;
use crate::i18n::{tl, tlf};
use crate::track_dialog::TrackKind;

use crate::file_dialogs::{Then, ask};
use craft_dialogs::FileDialog;
use disccraft_project::{AssetId, TimelineId};
use serde_json::{Value, json};

/// 1 つの字幕（秒）。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SubtitleCue {
    pub(crate) start: f64,
    pub(crate) end: f64,
    pub(crate) text: String,
    /// The subtitle's own colour group, stroke and place.
    pub(crate) look: disccraft_project::CueLook,
    /// The picture it shows instead of text (an image script's subtitle).
    pub(crate) image: Option<disccraft_project::CueImage>,
}

/// The subtitle shown over a picture: its text or its picture, and the colours and look it is drawn in.
#[derive(Clone, Debug)]
pub(crate) struct ShownSubtitle {
    pub(crate) text: String,
    pub(crate) colors: disccraft_project::SubtitleGroup,
    pub(crate) look: disccraft_project::CueLook,
    pub(crate) image: Option<disccraft_project::CueImage>,
}

/// A subtitle clip's name on the timeline: its first line, or a picture subtitle's file name.
pub(crate) fn cue_label(cue: &SubtitleCue) -> String {
    match &cue.image {
        Some(image) => Path::new(&image.path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        None => cue.text.lines().next().unwrap_or_default().to_owned(),
    }
}

/// A subtitle's look from a `subtitle.cues` entry (`group` counts from 1 there).
pub(crate) fn parse_look(cue: &Value) -> disccraft_project::CueLook {
    let small = |key: &str| {
        cue.get(key)
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok())
    };
    disccraft_project::CueLook {
        group: small("group").and_then(|group| group.checked_sub(1)),
        stroke: cue
            .get("stroke")
            .and_then(Value::as_str)
            .and_then(disccraft_project::SlideStroke::parse),
        align: small("align"),
        justify: cue
            .get("justify")
            .and_then(Value::as_str)
            .and_then(disccraft_project::SubtitleJustify::parse),
    }
}

/// 字幕ファイルの名前の最後の部分が言語コードなら取り出す（`movie.en.srt` → `en`）。
pub(crate) fn language_from_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let (_, code) = stem.rsplit_once('.')?;
    (code.len() == 2 && code.chars().all(|c| c.is_ascii_alphabetic()))
        .then(|| code.to_ascii_lowercase())
}

/// `cues` のうち `seconds` に出ている字幕。
pub(crate) fn cue_at(cues: &[SubtitleCue], seconds: f64) -> Option<&SubtitleCue> {
    cues.iter()
        .find(|cue| cue.start <= seconds && seconds < cue.end)
}

impl DiscCraftApp {
    /// 字幕アセットの字幕（読んだものを覚えておく。読めなかったときは空）。
    pub(crate) fn subtitle_cues(&mut self, asset: AssetId) -> Arc<Vec<SubtitleCue>> {
        if let Some(cues) = self.subtitle_cache.get(&asset.0) {
            return Arc::clone(cues);
        }
        // 読めなくても毎フレーム読み直さないように、空を覚える。理由はステータスに残る。
        let cues = self
            .run_query("subtitle.cues", json!({ "asset": asset.0 }))
            .and_then(|value| value.get("cues").and_then(Value::as_array).cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|cue| {
                Some(SubtitleCue {
                    start: cue.get("start")?.as_f64()?,
                    end: cue.get("end")?.as_f64()?,
                    text: cue.get("text")?.as_str()?.to_owned(),
                    look: parse_look(cue),
                    image: cue
                        .get("image")
                        .filter(|image| !image.is_null())
                        .and_then(|image| serde_json::from_value(image.clone()).ok()),
                })
            })
            .collect();
        let cues = Arc::new(cues);
        self.subtitle_cache.insert(asset.0, Arc::clone(&cues));
        cues
    }

    /// タイムラインの字幕トラックの字幕を読んでおく（タイムラインビューアが描く前に）。
    pub(crate) fn load_timeline_cues(&mut self, timeline: TimelineId) {
        let assets: Vec<AssetId> = self
            .session
            .project
            .timeline(timeline)
            .map(|timeline| timeline.subtitles.iter().map(|track| track.asset).collect())
            .unwrap_or_default();
        for asset in assets {
            let _ = self.subtitle_cues(asset);
        }
    }

    /// 読んである字幕（なければ `None`）。
    pub(crate) fn cached_cues(&self, asset: AssetId) -> Option<Arc<Vec<SubtitleCue>>> {
        self.subtitle_cache.get(&asset.0).map(Arc::clone)
    }

    /// モニターに重ねる字幕: タイムラインの、モニターに出す字幕トラックの `seconds` に出ているもの。
    pub(crate) fn monitor_subtitle(
        &mut self,
        timeline: TimelineId,
        seconds: f64,
    ) -> Option<ShownSubtitle> {
        let track = self.shown_subtitle(timeline)?;
        let asset = self
            .session
            .project
            .timeline(timeline)?
            .subtitles
            .get(track)?
            .asset;
        let cues = self.subtitle_cues(asset);
        let cue = cue_at(&cues, seconds)?;
        Some(ShownSubtitle {
            text: cue.text.clone(),
            colors: self.cue_colors(timeline, track, cue.look),
            look: cue.look,
            image: cue.image.clone(),
        })
    }

    /// Draws `shown` over `frame` (the whole picture; drawing is cut to `clip`): a picture subtitle as the disc
    /// shows it, its mapped colours in the colour group's at its place in the frame, else the text. Returns
    /// where it is drawn.
    pub(crate) fn draw_shown_subtitle(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        (frame, clip): (egui::Rect, egui::Rect),
        shown: &ShownSubtitle,
    ) -> egui::Rect {
        let Some(image) = &shown.image else {
            return crate::monitor::draw_subtitle(
                painter,
                frame.intersect(clip),
                &shown.text,
                (shown.colors, shown.look),
            );
        };
        let (width, height) = self.subtitle_frame();
        let Some((texture, [x, y, w, h])) =
            self.image_subtitle_texture(ctx, image, shown.colors, (width, height))
        else {
            return egui::Rect::NOTHING;
        };
        let scale = egui::vec2(
            frame.width() / f32::from(width.max(1)),
            frame.height() / f32::from(height.max(1)),
        );
        let at = egui::Rect::from_min_size(
            frame.min + egui::vec2(x * scale.x, y * scale.y),
            egui::vec2(w * scale.x, h * scale.y),
        );
        painter.with_clip_rect(clip).image(
            texture.id(),
            at,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        at
    }

    /// The frame a disc title's subtitles are drawn for: a Blu-ray's HD frame, else the SD frame.
    pub(crate) fn subtitle_frame(&self) -> (u16, u16) {
        let settings = &self.session.project.settings;
        match crate::preview_playback::disc_hd(settings) {
            Some((width, height, _, _)) => (
                u16::try_from(width).unwrap_or(1920),
                u16::try_from(height).unwrap_or(1080),
            ),
            None if settings.standard == disccraft_project::VideoStandard::Pal => (720, 576),
            None => (720, 480),
        }
    }

    /// A picture subtitle drawn as on the disc (in the group's colours), as a texture, and where it stands in the
    /// frame (x, y, width, height in the frame's pixels). Kept until the project changes; `None` (also kept)
    /// when the picture cannot be drawn, with the reason in the status line.
    fn image_subtitle_texture(
        &mut self,
        ctx: &egui::Context,
        image: &disccraft_project::CueImage,
        colors: disccraft_project::SubtitleGroup,
        (width, height): (u16, u16),
    ) -> Option<(egui::TextureHandle, [f32; 4])> {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (&image.path, image.area, width, height).hash(&mut hasher);
        let mapped = image.colors;
        (mapped.background, mapped.fill, mapped.outline).hash(&mut hasher);
        for color in [colors.fill, colors.stroke] {
            (color.rgb, color.opacity).hash(&mut hasher);
        }
        let key = hasher.finish();
        if let Some(found) = self.image_subtitles.get(&key) {
            return found.clone();
        }
        // A handful of pictures are on screen at a time; the rest are drawn again when they come back.
        if self.image_subtitles.len() >= 64 {
            self.image_subtitles.clear();
        }
        let made = match disccraft_menu::subtitle::render_image_cue(image, width, height) {
            Ok(bitmap) => {
                let (fill, stroke) = (
                    crate::monitor::subtitle_color(colors.fill),
                    crate::monitor::subtitle_color(colors.stroke),
                );
                let pixels: Vec<egui::Color32> = bitmap
                    .pixels
                    .iter()
                    .map(|value| match *value {
                        1 => fill,
                        2 => stroke,
                        _ => egui::Color32::TRANSPARENT,
                    })
                    .collect();
                let picture = egui::ColorImage::new(
                    [usize::from(bitmap.width), usize::from(bitmap.height)],
                    pixels,
                );
                let texture = ctx.load_texture(
                    format!("image-subtitle-{key:016x}"),
                    picture,
                    egui::TextureOptions::NEAREST,
                );
                Some((
                    texture,
                    [
                        f32::from(bitmap.x),
                        f32::from(bitmap.y),
                        f32::from(bitmap.width),
                        f32::from(bitmap.height),
                    ],
                ))
            }
            Err(error) => {
                self.status = error.to_string();
                None
            }
        };
        self.image_subtitles.insert(key, made.clone());
        made
    }

    /// 字幕の色: その字幕のグループ、なければトラックのグループの色。
    pub(crate) fn cue_colors(
        &self,
        timeline: TimelineId,
        track: usize,
        look: disccraft_project::CueLook,
    ) -> disccraft_project::SubtitleGroup {
        match look.group {
            Some(group) => self
                .session
                .project
                .timeline_subtitle_colors(timeline)
                .group(group),
            None => self.subtitle_track_colors(timeline, track),
        }
    }

    /// 字幕トラックのカラーグループの色（タイムラインのカラーセットから）。
    pub(crate) fn subtitle_track_colors(
        &self,
        timeline: TimelineId,
        track: usize,
    ) -> disccraft_project::SubtitleGroup {
        let group = self
            .session
            .project
            .timeline(timeline)
            .and_then(|found| found.subtitles.get(track))
            .map_or(0, |track| track.group);
        self.session
            .project
            .timeline_subtitle_colors(timeline)
            .group(group)
    }

    /// モニターに出す字幕トラック（なければ `None`）。
    pub(crate) fn shown_subtitle(&self, timeline: TimelineId) -> Option<usize> {
        let count = self
            .session
            .project
            .timeline(timeline)
            .map_or(0, |timeline| timeline.subtitles.len());
        shown_track(self.shown_subtitles.get(&timeline).copied(), count)
    }

    /// 字幕トラックの見出しの切り替え（Encore と同じ）: 押したトラックをモニターに出し、出ているトラックなら消す。
    pub(crate) fn toggle_shown_subtitle(&mut self, timeline: TimelineId, track: usize) {
        let shown = toggled(self.shown_subtitle(timeline), track);
        let _ = self.shown_subtitles.insert(timeline, shown);
    }

    /// タイムライン ▸ 字幕を読み込み…: ファイルを選んでアセットにし、字幕トラックの追加ダイアログを開く。
    pub(crate) fn import_subtitles(&mut self) {
        let Some(timeline) = self.selected_timeline() else {
            self.status = tl!("Select a timeline first").to_string();
            return;
        };
        ask(
            FileDialog::open().filter(
                tl!("Subtitles"),
                &[
                    "srt", "vtt", "txt", "ass", "ssa", "scc", "mcc", "stl", "ttml", "dfxp", "smi",
                    "sami",
                ],
            ),
            Then::ImportSubtitles(timeline),
        );
    }

    /// The subtitle file chosen for Timeline ▸ Import Subtitles ▸ Text Script… of `timeline`.
    pub(crate) fn import_subtitles_from(&mut self, timeline: TimelineId, path: &Path) {
        let imported = self.run_command("asset.import", crate::gui_params::import_asset(path));
        let Some(asset) = imported
            .and_then(|value| value.get("asset").and_then(Value::as_u64))
            .map(AssetId)
        else {
            return;
        };
        // 読めるか先に確かめる（読めなければ理由がステータスに出る）。
        self.subtitle_cache.remove(&asset.0);
        let count = self.subtitle_cues(asset).len();
        if count == 0 {
            return;
        }
        let language = language_from_name(path).unwrap_or_else(|| "ja".to_owned());
        self.open_track_dialog_with(timeline, TrackKind::Subtitle, asset, language);
        self.status = tlf!("Read {n} subtitles", n = count);
    }

    /// タイムライン ▸ 字幕を書き出し ▸ トラック: SRT（UTF-8）で保存する。
    pub(crate) fn export_subtitles(&mut self, asset: AssetId) {
        let name = self
            .session
            .project
            .assets
            .get(&asset)
            .map(|asset| asset.name.clone())
            .unwrap_or_else(|| "subtitles".to_owned());
        ask(
            FileDialog::save()
                .filter("SRT", &["srt"])
                // Encore の「字幕の書き出し ▸ テキストスクリプト」と同じ形式（Encore に読み込める）。
                .filter(tl!("Text Script"), &["txt"])
                .file_name(format!("{name}.srt")),
            Then::ExportSubtitles(asset),
        );
    }

    /// Writes the subtitles of `asset` to the file chosen for Export Subtitles.
    pub(crate) fn export_subtitles_to(&mut self, asset: AssetId, path: &Path) {
        let target = path.to_string_lossy().into_owned();
        if self
            .run_query(
                "subtitle.exportSrt",
                json!({ "asset": asset.0, "path": target }),
            )
            .is_some()
        {
            self.status = tlf!("Exported: {path}", path = target);
        }
    }
}

/// モニターに出すトラック: 選んだもの（`Some(None)` はなし）、選んでいなければ 1 本目。ないトラックは出さない。
fn shown_track(choice: Option<Option<usize>>, count: usize) -> Option<usize> {
    choice.unwrap_or(Some(0)).filter(|track| *track < count)
}

/// 見出しを押したあとのトラック: 出ているトラックなら消し、ほかはそのトラックに替える。
fn toggled(shown: Option<usize>, track: usize) -> Option<usize> {
    (shown != Some(track)).then_some(track)
}

#[cfg(test)]
mod tests {
    use super::{SubtitleCue, cue_at, language_from_name, shown_track, toggled};

    #[test]
    fn one_subtitle_track_or_none_is_shown() {
        // Nothing chosen: the first track, when there is one.
        assert_eq!(shown_track(None, 2), Some(0));
        assert_eq!(shown_track(None, 0), None);
        // A chosen track, or none at all; a track that is gone shows nothing.
        assert_eq!(shown_track(Some(Some(1)), 2), Some(1));
        assert_eq!(shown_track(Some(None), 2), None);
        assert_eq!(shown_track(Some(Some(2)), 2), None);
        // Clicking another track moves the display there; clicking the shown one turns it off.
        assert_eq!(toggled(Some(1), 0), Some(0));
        assert_eq!(toggled(Some(0), 0), None);
        assert_eq!(toggled(None, 1), Some(1));
    }
    use std::path::Path;

    #[test]
    fn language_comes_from_the_file_name() {
        assert_eq!(
            language_from_name(Path::new("movie.en.srt")),
            Some("en".to_owned())
        );
        assert_eq!(
            language_from_name(Path::new("movie.JA.vtt")),
            Some("ja".to_owned())
        );
        assert_eq!(language_from_name(Path::new("movie.srt")), None);
        assert_eq!(language_from_name(Path::new("movie.eng.srt")), None);
    }

    #[test]
    fn the_cue_on_screen_is_found() {
        let cues = vec![
            SubtitleCue {
                start: 1.0,
                end: 2.0,
                text: "a".to_owned(),
                look: disccraft_project::CueLook::default(),
                image: None,
            },
            SubtitleCue {
                start: 3.0,
                end: 4.0,
                text: "b".to_owned(),
                look: disccraft_project::CueLook::default(),
                image: None,
            },
        ];
        assert_eq!(cue_at(&cues, 1.5).map(|cue| cue.text.as_str()), Some("a"));
        assert_eq!(cue_at(&cues, 2.0), None);
        assert_eq!(cue_at(&cues, 3.0).map(|cue| cue.text.as_str()), Some("b"));
    }
}
