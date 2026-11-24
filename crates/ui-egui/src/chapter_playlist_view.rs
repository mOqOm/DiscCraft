//! チャプター再生リストビューア（Encore と同じ形。[docs/ui-parity.md](../../../docs/ui-parity.md) §3.5）。
//!
//! 左に「マスタータイムライン: <名前>」の表（チャプター番号 / チャプター名 / イン）、右に「チャプター再生リスト:
//! <名前>」の表（エントリ番号 / チャプター番号 / チャプター名 / イン）。左の行をダブルクリックするか → で右に
//! 加え、削除 で外す。下端にデュレーションと 再生の種類（連続 / ランダム / シャッフル）。

use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf};

use disccraft_project::{ChapterPlaylist, PlaybackOrder, Timeline, format_time};
use egui::{Ui, vec2};

/// 2 つの表の間の列の幅。
const MIDDLE_WIDTH: f32 = 72.0;

impl DiscCraftApp {
    /// チャプター再生リストビューア。
    pub(crate) fn chapter_playlist_view(&mut self, ui: &mut Ui, id: u64) {
        let Some(playlist) = self.session.project.chapter_playlist(id).cloned() else {
            ui.label(tl!("Chapter playlist not found"));
            return;
        };
        let Some(timeline) = self.session.project.timeline(playlist.timeline).cloned() else {
            ui.label(tl!(
                "The master timeline of this chapter playlist no longer exists."
            ));
            return;
        };
        let spans = self.chapter_spans(&timeline);
        let total = entry_seconds(&playlist, &spans);
        let footer = ui.spacing().interact_size.y + 16.0;
        let table_height = (ui.available_height() - footer).max(80.0);
        let table_width = ((ui.available_width() - MIDDLE_WIDTH - 24.0) / 2.0).max(160.0);

        ui.horizontal_top(|ui| {
            ui.allocate_ui(vec2(table_width, table_height), |ui| {
                ui.vertical(|ui| {
                    ui.strong(tlf!("Master Timeline: {name}", name = timeline.name));
                    self.master_chapter_table(ui, id, &timeline);
                });
            });
            ui.allocate_ui(vec2(MIDDLE_WIDTH, table_height), |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(table_height * 0.4);
                    self.middle_buttons(ui, id, &playlist);
                });
            });
            ui.allocate_ui(vec2(table_width, table_height), |ui| {
                ui.vertical(|ui| {
                    ui.strong(tlf!("Chapter Playlist: {name}", name = playlist.name));
                    self.entry_table(ui, id, &playlist, &timeline);
                });
            });
        });

        ui.separator();
        ui.horizontal(|ui| {
            ui.label(tlf!("Duration: {time}", time = self.duration_text(total)));
            ui.add_space(12.0);
            ui.label(tl!("Playback:"));
            self.playback_order_combo(ui, id, playlist.order, "viewer");
        });
    }

    /// The master chapters picked for chapter playlist `id`, in chapter order.
    fn picked_chapters(&self, id: u64) -> Vec<usize> {
        if self
            .chapter_playlist_pick
            .is_none_or(|(list, _)| list != id)
        {
            return Vec::new();
        }
        let mut picks = self.chapter_playlist_picks.clone();
        picks.sort_unstable();
        picks.dedup();
        picks
    }

    /// 左の表: マスタータイムラインのチャプター。行のどこを押しても選び（M6.425: 行全体が選択の色。Shift+クリックで
    /// 範囲、Ctrl+クリックで 1 つずつ足す・外す、表の上での Ctrl+A ですべて）、ダブルクリックでその行を右に加える。
    fn master_chapter_table(&mut self, ui: &mut Ui, id: u64, timeline: &Timeline) {
        let settings = self.session.project.settings.clone();
        let picks = self.picked_chapters(id);
        let mut clicked = None;
        let mut added = None;
        let scroll = egui::ScrollArea::vertical()
            .id_salt(("master_chapters", id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                crate::theme::table_selecting_rows(("master_chapter_grid", id), picks.clone())
                    .num_columns(3)
                    .min_col_width(60.0)
                    .show(ui, |ui| {
                        ui.strong(tl!("Chapter #"));
                        ui.strong(tl!("Chapter Name"));
                        ui.strong(tl!("In"));
                        ui.end_row();
                        for (index, chapter) in timeline.chapters.iter().enumerate() {
                            let first = ui.label(index.saturating_add(1).to_string()).rect;
                            let _ =
                                ui.label(crate::timeline_view::chapter_label(&chapter.name, index));
                            let last = ui.label(format_time(chapter.time, &settings)).rect;
                            let row = crate::theme::table_row(
                                ui,
                                egui::Id::unique("master_chapter_row").with((id, index)),
                                first,
                                last,
                            );
                            if row.double_clicked() {
                                added = Some(index);
                            } else if row.clicked() {
                                clicked = Some(index);
                            }
                            ui.end_row();
                        }
                    });
            });
        let (shift, command) = ui.input(|input| (input.modifiers.shift, input.modifiers.command));
        if let Some(index) = clicked {
            let anchor = self
                .chapter_playlist_pick
                .filter(|(list, _)| *list == id)
                .map(|(_, anchor)| anchor);
            match (shift, command, anchor) {
                (true, _, Some(anchor)) => {
                    self.chapter_playlist_picks = (anchor.min(index)..=anchor.max(index)).collect();
                }
                (false, true, Some(_)) => {
                    let mut picks = picks.clone();
                    if let Some(at) = picks.iter().position(|pick| *pick == index) {
                        let _ = picks.remove(at);
                    } else {
                        picks.push(index);
                    }
                    self.chapter_playlist_picks = picks;
                    self.chapter_playlist_pick = Some((id, index));
                }
                _ => {
                    self.chapter_playlist_picks = vec![index];
                    self.chapter_playlist_pick = Some((id, index));
                }
            }
        }
        // Ctrl+A over the table: every chapter.
        let select_all = ui.rect_contains_pointer(scroll.inner_rect)
            && ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::A));
        if select_all && !timeline.chapters.is_empty() {
            self.chapter_playlist_picks = (0..timeline.chapters.len()).collect();
            if self
                .chapter_playlist_pick
                .is_none_or(|(list, _)| list != id)
            {
                self.chapter_playlist_pick = Some((id, 0));
            }
        }
        if let Some(index) = added {
            self.chapter_playlist_picks = vec![index];
            self.chapter_playlist_pick = Some((id, index));
            self.add_chapter_playlist_entries(id, &[index]);
        }
    }

    /// 右の表: 再生リストのエントリ。行のどこを押しても選び（行全体が選択の色）、ダブルクリックで外す。
    fn entry_table(
        &mut self,
        ui: &mut Ui,
        id: u64,
        playlist: &ChapterPlaylist,
        timeline: &Timeline,
    ) {
        let settings = self.session.project.settings.clone();
        let selected = self
            .chapter_entry_pick
            .filter(|(list, _)| *list == id)
            .map(|(_, entry)| entry);
        let mut picked = None;
        let mut removed = None;
        egui::ScrollArea::vertical()
            .id_salt(("playlist_entries", id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                crate::theme::table_selecting_rows(
                    ("playlist_entry_grid", id),
                    selected.into_iter().collect(),
                )
                .num_columns(4)
                .min_col_width(60.0)
                .show(ui, |ui| {
                    ui.strong(tl!("Entry #"));
                    ui.strong(tl!("Chapter #"));
                    ui.strong(tl!("Chapter Name"));
                    ui.strong(tl!("In"));
                    ui.end_row();
                    for (entry, chapter) in playlist.chapters.iter().enumerate() {
                        let index = usize::try_from(*chapter).unwrap_or(usize::MAX);
                        let point = timeline.chapters.get(index);
                        let first = ui.label(entry.saturating_add(1).to_string()).rect;
                        let _ = ui.label(index.saturating_add(1).to_string());
                        let _ = ui.label(point.map_or_else(
                            || tl!("(missing chapter)").to_string(),
                            |point| crate::timeline_view::chapter_label(&point.name, index),
                        ));
                        let last = ui
                            .label(point.map_or_else(String::new, |point| {
                                format_time(point.time, &settings)
                            }))
                            .rect;
                        let row = crate::theme::table_row(
                            ui,
                            egui::Id::unique("playlist_entry_row").with((id, entry)),
                            first,
                            last,
                        );
                        if row.double_clicked() {
                            removed = Some(entry);
                        } else if row.clicked() {
                            picked = Some(entry);
                        }
                        ui.end_row();
                    }
                });
            });
        if let Some(entry) = picked {
            self.chapter_entry_pick = Some((id, entry));
        }
        if let Some(entry) = removed {
            self.remove_chapter_playlist_entry(id, entry);
        }
    }

    /// 間の列: 加える（→）、削除、上へ / 下へ（Encore の 再生の順番）。
    fn middle_buttons(&mut self, ui: &mut Ui, id: u64, playlist: &ChapterPlaylist) {
        let picks = self.picked_chapters(id);
        let entry = self
            .chapter_entry_pick
            .filter(|(list, _)| *list == id)
            .map(|(_, entry)| entry)
            .filter(|entry| *entry < playlist.chapters.len());
        if ui
            .add_enabled(!picks.is_empty(), egui::Button::new(tl!("Add →")))
            .on_hover_text(tl!(
                "Add the selected chapters to the playlist in chapter order (or double-click one). Shift+click picks a range, Ctrl+click one more, Ctrl+A every chapter"
            ))
            .clicked()
        {
            self.add_chapter_playlist_entries(id, &picks);
        }
        if ui
            .add_enabled(entry.is_some(), egui::Button::new(tl!("Remove")))
            .on_hover_text(tl!("Remove the selected entry (or double-click it)"))
            .clicked()
            && let Some(entry) = entry
        {
            self.remove_chapter_playlist_entry(id, entry);
        }
        ui.add_space(8.0);
        let can_up = entry.is_some_and(|entry| entry > 0);
        let can_down = entry.is_some_and(|entry| entry.saturating_add(1) < playlist.chapters.len());
        if ui
            .add_enabled(can_up, egui::Button::new(tl!("Up")))
            .clicked()
            && let Some(entry) = entry
        {
            self.move_chapter_playlist_entry(id, entry, entry.saturating_sub(1));
        }
        if ui
            .add_enabled(can_down, egui::Button::new(tl!("Down")))
            .clicked()
            && let Some(entry) = entry
        {
            self.move_chapter_playlist_entry(id, entry, entry.saturating_add(1));
        }
    }

    /// 再生の種類のコンボ（連続 / ランダム / シャッフル）。
    pub(crate) fn playback_order_combo(
        &mut self,
        ui: &mut Ui,
        id: u64,
        current: PlaybackOrder,
        salt: &str,
    ) {
        let mut chosen = None;
        egui::ComboBox::from_id_salt(("playback_order", id, salt))
            .selected_text(playback_order_label(current))
            .show_ui(ui, |ui| {
                for order in PlaybackOrder::ALL {
                    if ui
                        .selectable_label(order == current, playback_order_label(order))
                        .clicked()
                    {
                        chosen = Some(order);
                    }
                }
            });
        if let Some(order) = chosen
            && order != current
        {
            let _ = self.run_command(
                "chapterplaylist.setOrder",
                serde_json::json!({ "chapterPlaylist": id, "order": order.as_str() }),
            );
        }
    }

    /// Adds `chapters` to the playlist in one go (one undo, M6.425) and selects the last entry added.
    fn add_chapter_playlist_entries(&mut self, id: u64, chapters: &[usize]) {
        let added = self.run_command(
            "chapterplaylist.addChapter",
            serde_json::json!({ "chapterPlaylist": id, "chapters": chapters }),
        );
        if let Some(entry) = added
            .as_ref()
            .and_then(|value| value.get("entries"))
            .and_then(serde_json::Value::as_array)
            .and_then(|entries| entries.last())
            .and_then(serde_json::Value::as_u64)
            .and_then(|entry| usize::try_from(entry).ok())
        {
            self.chapter_entry_pick = Some((id, entry));
        }
    }

    fn remove_chapter_playlist_entry(&mut self, id: u64, entry: usize) {
        let removed = self.run_command(
            "chapterplaylist.removeEntry",
            serde_json::json!({ "chapterPlaylist": id, "entry": entry }),
        );
        if removed.is_some() {
            let remaining = self
                .session
                .project
                .chapter_playlist(id)
                .map_or(0, |list| list.chapters.len());
            self.chapter_entry_pick = (remaining > 0).then(|| (id, entry.min(remaining - 1)));
        }
    }

    fn move_chapter_playlist_entry(&mut self, id: u64, from: usize, to: usize) {
        let moved = self.run_command(
            "chapterplaylist.moveEntry",
            serde_json::json!({ "chapterPlaylist": id, "from": from, "to": to }),
        );
        if moved.is_some() {
            self.chapter_entry_pick = Some((id, to));
        }
    }

    /// 各チャプターの長さ（秒）: 次のチャプターまで、最後は本編の終わりまで（長さが分からなければ 0）。
    pub(crate) fn chapter_spans(&mut self, timeline: &Timeline) -> Vec<f64> {
        let duration = self.timeline_seconds(timeline.id);
        let starts: Vec<f64> = timeline
            .chapters
            .iter()
            .map(|chapter| crate::timeline_view::ticks_to_seconds(chapter.time.0))
            .collect();
        starts
            .iter()
            .enumerate()
            .map(|(index, start)| {
                let end = starts
                    .get(index.saturating_add(1))
                    .copied()
                    .unwrap_or(duration);
                (end - start).max(0.0)
            })
            .collect()
    }

    /// チャプター再生リストの長さ（秒）: エントリのチャプターの長さの合計。
    pub(crate) fn chapter_playlist_seconds(&mut self, id: u64) -> Option<f64> {
        let playlist = self.session.project.chapter_playlist(id).cloned()?;
        let timeline = self.session.project.timeline(playlist.timeline).cloned()?;
        let spans = self.chapter_spans(&timeline);
        Some(entry_seconds(&playlist, &spans))
    }
}

/// The entries' total length from the per-chapter spans.
fn entry_seconds(playlist: &ChapterPlaylist, spans: &[f64]) -> f64 {
    playlist
        .chapters
        .iter()
        .filter_map(|chapter| usize::try_from(*chapter).ok())
        .filter_map(|index| spans.get(index))
        .sum()
}

/// Encore の 再生の種類 の表示名。
pub(crate) fn playback_order_label(order: PlaybackOrder) -> &'static str {
    match order {
        PlaybackOrder::Sequential => tl!("Sequential"),
        PlaybackOrder::Random => tl!("Random"),
        PlaybackOrder::Shuffle => tl!("Shuffle"),
    }
}

#[cfg(test)]
mod tests {
    use super::entry_seconds;
    use disccraft_project::{ChapterPlaylist, Link, PlaybackOrder, TimelineId};

    #[test]
    fn entry_seconds_sum_the_chapter_spans_and_skip_missing_chapters() {
        let playlist = ChapterPlaylist {
            parental_level: None,
            id: 1,
            name: String::new(),
            timeline: TimelineId(1),
            chapters: vec![2, 0, 2, 9],
            end_action: Link::NotSet,
            override_link: None,
            description: String::new(),
            prohibited_ops: Vec::new(),
            order: PlaybackOrder::Sequential,
        };
        let total = entry_seconds(&playlist, &[10.0, 20.0, 5.5]);
        assert!((total - 21.0).abs() < 1e-9);
    }
}
