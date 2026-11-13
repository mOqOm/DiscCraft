//! 字幕ファイル: タイムライン ▸ 字幕を読み込み… / 字幕を書き出し、タイムラインの字幕トラックの字幕の表示、
//! モニターに今の字幕を重ねる。読み込みは engine の `subtitle.cues`（SRT、WebVTT、タイムコード付きテキスト。
//! UTF-8 / UTF-16 / Shift_JIS）。

use std::path::Path;
use std::sync::Arc;

use crate::DiscCraftApp;
use crate::i18n::{tl, tlf};
use crate::track_dialog::TrackKind;

use disccraft_project::{AssetId, TimelineId};
use rfd::FileDialog;
use serde_json::{Value, json};

/// 1 つの字幕（秒）。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SubtitleCue {
    pub(crate) start: f64,
    pub(crate) end: f64,
    pub(crate) text: String,
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
    ) -> Option<String> {
        let track = self.shown_subtitle(timeline)?;
        let asset = self
            .session
            .project
            .timeline(timeline)?
            .subtitles
            .get(track)?
            .asset;
        let cues = self.subtitle_cues(asset);
        cue_at(&cues, seconds).map(|cue| cue.text.clone())
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
        let Some(path) = FileDialog::new()
            .add_filter(tl!("Subtitles"), &["srt", "vtt", "txt"])
            .pick_file()
        else {
            return;
        };
        let imported = self.run_command("asset.import", crate::gui_params::import_asset(&path));
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
        let language = language_from_name(&path).unwrap_or_else(|| "ja".to_owned());
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
        let Some(path) = FileDialog::new()
            .add_filter("SRT", &["srt"])
            .set_file_name(format!("{name}.srt"))
            .save_file()
        else {
            return;
        };
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
            },
            SubtitleCue {
                start: 3.0,
                end: 4.0,
                text: "b".to_owned(),
            },
        ];
        assert_eq!(cue_at(&cues, 1.5).map(|cue| cue.text.as_str()), Some("a"));
        assert_eq!(cue_at(&cues, 2.0), None);
        assert_eq!(cue_at(&cues, 3.0).map(|cue| cue.text.as_str()), Some("b"));
    }
}
