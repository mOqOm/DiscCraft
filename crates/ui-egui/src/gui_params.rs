//! GUI が engine コマンドへ渡すパラメータの組み立て。
//!
//! 組み立てだけをここに切り出し、`Session` への実行テストで
//! 「GUI が送るパラメータを engine が受け付ける」ことを検査する
//! （[docs/ui-parity.md](../../../docs/ui-parity.md) §5）。

use disccraft_project::{Align, Axis, Link, Rect};
use serde_json::{Map, Value, json};
use std::path::Path;

/// `timeline.create` のパラメータ。
pub(crate) fn timeline_create(asset: u64) -> Value {
    json!({ "asset": asset })
}

/// `timeline.addAudioTrack` のパラメータ。
pub(crate) fn add_audio_track(timeline: u64, asset: u64, language: &str) -> Value {
    json!({ "timeline": timeline, "asset": asset, "language": language })
}

/// `timeline.addSubtitleTrack` のパラメータ。
pub(crate) fn add_subtitle_track(timeline: u64, asset: u64, language: &str) -> Value {
    json!({ "timeline": timeline, "asset": asset, "language": language })
}

/// `timeline.removeAudioTrack` / `timeline.removeSubtitleTrack` のパラメータ。
pub(crate) fn remove_track(timeline: u64, track: usize) -> Value {
    let track = u64::try_from(track).unwrap_or(u64::MAX);
    json!({ "timeline": timeline, "track": track })
}

/// `button.setLabel` のパラメータ。
pub(crate) fn button_label(menu: u64, button: usize, label: &str) -> Value {
    let button = u64::try_from(button).unwrap_or(u64::MAX);
    json!({ "menu": menu, "button": button, "label": label })
}

/// リンクを受け取るコマンド（`button.setLink`、`timeline.setEndAction`、`nav.setFirstPlay`）のパラメータ。
///
/// engine 側のキー名は `target`（`docs/architecture.md` §7）。
pub(crate) fn with_target(mut base: Map<String, Value>, link: &Link) -> Result<Value, String> {
    let value = serde_json::to_value(link).map_err(|error| error.to_string())?;
    base.insert("target".to_string(), value);
    Ok(Value::Object(base))
}

/// `menu.setBackground` の単色背景。engine は `"#RRGGBB"` の文字列を受ける。
pub(crate) fn background_color(menu: u64, rgb: [u8; 3]) -> Value {
    json!({ "menu": menu, "color": crate::app::rgb_to_hex(rgb) })
}

/// `menu.setBackground` の画像背景。
pub(crate) fn background_image(menu: u64, asset: u64) -> Value {
    json!({ "menu": menu, "asset": asset })
}

/// `menu.setColors` のパラメータ。
#[cfg(test)]
pub(crate) fn menu_colors(menu: u64, text: [u8; 3], highlight: [u8; 3]) -> Value {
    json!({
        "menu": menu,
        "text": crate::app::rgb_to_hex(text),
        "highlight": crate::app::rgb_to_hex(highlight),
    })
}

/// `asset.import` のパラメータ。
pub(crate) fn import_asset(path: &Path) -> Value {
    json!({ "path": path.to_string_lossy().to_string() })
}

/// `project.rename` のパラメータ。
pub(crate) fn rename(kind: &str, id: u64, name: &str) -> Value {
    json!({ "kind": kind, "id": id, "name": name })
}

/// `slideshow.create` / `playlist.create` のパラメータ。
pub(crate) fn create_named(name: &str) -> Value {
    json!({ "name": name })
}

/// `slideshow.addSlide` のパラメータ。
pub(crate) fn add_slide(slideshow: u64, asset: u64, seconds: Option<f64>) -> Value {
    match seconds {
        Some(seconds) => json!({ "slideshow": slideshow, "asset": asset, "seconds": seconds }),
        None => json!({ "slideshow": slideshow, "asset": asset }),
    }
}

/// `slideshow.removeSlide` のパラメータ。
pub(crate) fn remove_slide(slideshow: u64, index: usize) -> Value {
    let index = u64::try_from(index).unwrap_or(u64::MAX);
    json!({ "slideshow": slideshow, "slide": index })
}

/// `playlist.addItem` のパラメータ。
pub(crate) fn playlist_entry(playlist: u64, item: disccraft_project::PlaylistItem) -> Value {
    use disccraft_project::PlaylistItem;
    match item {
        PlaylistItem::Timeline(id) => json!({ "playlist": playlist, "timeline": id.0 }),
        PlaylistItem::Slideshow(id) => json!({ "playlist": playlist, "slideshow": id }),
        PlaylistItem::ChapterPlaylist(id) => {
            json!({ "playlist": playlist, "chapterPlaylist": id })
        }
    }
}

/// `playlist.removeItem` のパラメータ。
pub(crate) fn remove_playlist_item(playlist: u64, index: usize) -> Value {
    let index = u64::try_from(index).unwrap_or(u64::MAX);
    json!({ "playlist": playlist, "item": index })
}

/// `menu.setDefaultButton` のパラメータ。
pub(crate) fn set_default_button(menu: u64, button: usize) -> Value {
    let button = u64::try_from(button).unwrap_or(u64::MAX);
    json!({ "menu": menu, "button": button })
}

/// `button.setRect` のパラメータ。
pub(crate) fn set_button_rect(menu: u64, button: usize, rect: Rect) -> Value {
    let button = u64::try_from(button).unwrap_or(u64::MAX);
    json!({
        "menu": menu,
        "button": button,
        "rect": { "x": rect.x, "y": rect.y, "w": rect.w, "h": rect.h },
    })
}

/// 矩形の JSON（`{"x", "y", "w", "h"}`）。
fn rect_json(rect: Rect) -> Value {
    json!({ "x": rect.x, "y": rect.y, "w": rect.w, "h": rect.h })
}

/// 番号の並びの JSON。
fn index_json(indexes: &[usize]) -> Vec<u64> {
    indexes
        .iter()
        .map(|index| u64::try_from(*index).unwrap_or(u64::MAX))
        .collect()
}

/// `menu.pasteButtons` のパラメータ。
pub(crate) fn paste_buttons(menu: u64, buttons: &[crate::button_edit::ClipButton]) -> Value {
    let buttons: Vec<Value> = buttons
        .iter()
        .map(|button| {
            let target = serde_json::to_value(&button.target).unwrap_or(Value::Null);
            json!({
                "label": button.label,
                "rect": rect_json(button.rect),
                "target": target,
                "autoActivate": button.auto_activate,
            })
        })
        .collect();
    json!({ "menu": menu, "buttons": buttons })
}

/// `menu.removeButtons` のパラメータ。
pub(crate) fn button_indexes(menu: u64, buttons: &[usize]) -> Value {
    json!({ "menu": menu, "buttons": index_json(buttons) })
}

/// `button.setRects` のパラメータ。
pub(crate) fn set_button_rects(menu: u64, rects: &[(usize, Rect)]) -> Value {
    let rects: Vec<Value> = rects
        .iter()
        .map(|(index, rect)| {
            let index = u64::try_from(*index).unwrap_or(u64::MAX);
            json!({ "button": index, "rect": rect_json(*rect) })
        })
        .collect();
    json!({ "menu": menu, "rects": rects })
}

/// `button.setAutoActivate` のパラメータ。
pub(crate) fn auto_activate(menu: u64, button: usize, on: bool) -> Value {
    json!({ "menu": menu, "button": index_json(&[button]).first(), "on": on })
}

/// `menu.setDuration` のパラメータ（`None` は静止）。
pub(crate) fn menu_duration(menu: u64, seconds: Option<u32>) -> Value {
    json!({ "menu": menu, "seconds": seconds })
}

/// `menu.reorderButtons` のパラメータ。
pub(crate) fn reorder_buttons(menu: u64, order: &[usize]) -> Value {
    json!({ "menu": menu, "order": index_json(order) })
}

/// `menu.alignButtons` のパラメータ。
pub(crate) fn align_buttons(menu: u64, buttons: &[usize], align: Align, safe_area: bool) -> Value {
    json!({
        "menu": menu,
        "buttons": index_json(buttons),
        "align": align.name(),
        "safeArea": safe_area,
    })
}

/// `menu.distributeButtons` のパラメータ。
pub(crate) fn distribute_buttons(
    menu: u64,
    buttons: &[usize],
    axis: Axis,
    safe_area: bool,
) -> Value {
    json!({
        "menu": menu,
        "buttons": index_json(buttons),
        "axis": axis.name(),
        "safeArea": safe_area,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use disccraft_engine::Session;
    use disccraft_project::{Link, MenuId, Severity, TimelineId};
    use serde_json::Value;
    use std::error::Error;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> Result<PathBuf, Box<dyn Error>> {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "disccraft-gui-params-{}-{name}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    fn fake_file(dir: &Path, name: &str) -> Result<PathBuf, Box<dyn Error>> {
        let path = dir.join(name);
        fs::write(&path, b"fake")?;
        Ok(path)
    }

    fn id(value: &Value, key: &str) -> Result<u64, Box<dyn Error>> {
        value
            .get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("missing {key}").into())
    }

    fn import(session: &mut Session, dir: &Path, name: &str) -> Result<u64, Box<dyn Error>> {
        let path = fake_file(dir, name)?;
        let result = session.execute("asset.import", &import_asset(&path))?;
        id(&result, "asset")
    }

    /// GUI のプロジェクトパネル → タイムライン → メニュー → リンク → ディスク設定の流れが
    /// コマンドレベルで通ることを検査する（M6.5）。
    #[test]
    fn gui_params_are_accepted_by_the_engine() -> Result<(), Box<dyn Error>> {
        let dir = temp_dir("flow")?;
        let mut session = Session::new();

        let video = import(&mut session, &dir, "movie.mp4")?;
        let audio = import(&mut session, &dir, "voice.wav")?;
        let background = import(&mut session, &dir, "bg.png")?;

        let created = session.execute("timeline.create", &timeline_create(video))?;
        let timeline = id(&created, "timeline")?;

        session.execute(
            "timeline.addAudioTrack",
            &add_audio_track(timeline, audio, "ja"),
        )?;

        let subtitles = import(&mut session, &dir, "ja.srt")?;
        session.execute(
            "timeline.addSubtitleTrack",
            &add_subtitle_track(timeline, subtitles, "ja"),
        )?;
        session.execute("timeline.removeSubtitleTrack", &remove_track(timeline, 0))?;
        session.execute("timeline.removeAudioTrack", &remove_track(timeline, 0))?;
        session.execute(
            "timeline.addAudioTrack",
            &add_audio_track(timeline, audio, "ja"),
        )?;

        let added = session.execute(
            "timeline.addChapter",
            &json!({ "timeline": timeline, "timecode": "00:01:00:00" }),
        )?;
        assert_eq!(id(&added, "chapter")?, 1);

        let menu_created = session.execute("menu.create", &json!({ "name": "メイン" }))?;
        let menu = id(&menu_created, "menu")?;
        session.execute("menu.addButton", &json!({ "menu": menu, "label": "再生" }))?;

        session.execute("button.setLabel", &button_label(menu, 0, "本編再生"))?;

        let link = Link::Timeline {
            id: TimelineId(timeline),
            chapter: 1,
        };
        let params = with_target(
            Map::from_iter([
                ("menu".to_string(), Value::from(menu)),
                ("button".to_string(), Value::from(0)),
            ]),
            &link,
        )?;
        session.execute("button.setLink", &params)?;

        session.execute("menu.addButton", &json!({ "menu": menu, "label": "終了" }))?;
        session.execute("menu.removeButtons", &button_indexes(menu, &[1]))?;

        let copies = [crate::button_edit::ClipButton {
            label: "コピー".to_string(),
            rect: Rect {
                x: 40,
                y: 40,
                w: 120,
                h: 40,
            },
            target: Link::Stop,
            auto_activate: true,
        }];
        session.execute("menu.pasteButtons", &paste_buttons(menu, &copies))?;
        session.execute(
            "button.setRects",
            &set_button_rects(
                menu,
                &[
                    (
                        0,
                        Rect {
                            x: 40,
                            y: 300,
                            w: 200,
                            h: 40,
                        },
                    ),
                    (
                        1,
                        Rect {
                            x: 300,
                            y: 300,
                            w: 120,
                            h: 40,
                        },
                    ),
                ],
            ),
        )?;
        session.execute(
            "menu.alignButtons",
            &align_buttons(menu, &[0, 1], Align::Top, false),
        )?;
        session.execute(
            "menu.alignButtons",
            &align_buttons(menu, &[1], Align::HorizontalCenter, true),
        )?;
        session.execute(
            "menu.distributeButtons",
            &distribute_buttons(menu, &[0, 1], Axis::Horizontal, true),
        )?;
        session.execute("menu.reorderButtons", &reorder_buttons(menu, &[1, 0]))?;
        session.execute("button.setAutoActivate", &auto_activate(menu, 0, true))?;
        session.execute("menu.removeButtons", &button_indexes(menu, &[1]))?;

        session.execute("menu.setBackground", &background_image(menu, background))?;
        session.execute("menu.setBackground", &background_color(menu, [16, 32, 64]))?;
        session.execute(
            "menu.setColors",
            &menu_colors(menu, [255, 255, 255], [255, 208, 0]),
        )?;

        let end_params = with_target(
            Map::from_iter([("timeline".to_string(), Value::from(timeline))]),
            &Link::LastMenu,
        )?;
        session.execute("timeline.setEndAction", &end_params)?;
        let remote_params = with_target(
            Map::from_iter([("timeline".to_string(), Value::from(timeline))]),
            &Link::Menu {
                id: MenuId(menu),
                button: None,
            },
        )?;
        session.execute("timeline.setMenuRemote", &remote_params)?;
        let menu_end = with_target(
            Map::from_iter([("menu".to_string(), Value::from(menu))]),
            &Link::Timeline {
                id: TimelineId(timeline),
                chapter: 0,
            },
        )?;
        session.execute("menu.setEndAction", &menu_end)?;
        session.execute("menu.setDuration", &menu_duration(menu, Some(30)))?;
        session.execute("menu.setDuration", &menu_duration(menu, None))?;
        assert!(
            session
                .execute("menu.setDuration", &menu_duration(menu, Some(255)))
                .is_err()
        );

        session.execute(
            "nav.setFirstPlay",
            &with_target(
                Map::new(),
                &Link::Menu {
                    id: MenuId(menu),
                    button: None,
                },
            )?,
        )?;

        // メディアのプローブは実ファイルが必要なので、文書モデルの不変条件
        // （リンク先、チャプター順、上限）が壊れていないことをここで確かめる。
        let problems = session.project.check();
        let structural: Vec<String> = problems
            .iter()
            .filter(|problem| problem.severity == Severity::Error)
            .map(|problem| problem.message.clone())
            .collect();
        assert!(structural.is_empty(), "structural problems: {structural:?}");

        fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn colors_are_sent_as_hex_strings() {
        let params = background_color(3, [255, 0, 128]);
        assert_eq!(params.get("color").and_then(Value::as_str), Some("#ff0080"));

        let params = menu_colors(3, [1, 2, 3], [4, 5, 6]);
        assert_eq!(params.get("text").and_then(Value::as_str), Some("#010203"));
        assert_eq!(
            params.get("highlight").and_then(Value::as_str),
            Some("#040506")
        );
    }

    #[test]
    fn link_key_is_target() -> Result<(), Box<dyn Error>> {
        let params = with_target(
            Map::from_iter([("menu".to_string(), Value::from(4u64))]),
            &Link::Stop,
        )?;

        assert_eq!(params.get("link").and_then(Value::as_str), None);
        assert_eq!(params.get("target").and_then(Value::as_str), Some("stop"));
        Ok(())
    }
}
