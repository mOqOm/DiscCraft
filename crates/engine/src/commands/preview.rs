//! ディスクの VM プレビュー（`preview.*`）。
//!
//! プレビューはディスクのナビゲーションを [`disccraft_vm`] で動かし、リモコン操作だけを受け取ります。
//! ディスクは 3 通りで開きます: ビルドせずにプロジェクトから（Encore のプロジェクトプレビュー。ビルドが
//! 書くのと同じモデルを `disccraft_dvd::preview_model` で作る）、ビルドした `VIDEO_TS` から、ビルドした `BDMV`
//! から（HDMV の解釈器 [`disccraft_vm::BdVm`]。プレイリストをプロジェクトのタイムラインとメニューに対応させて、
//! 画面には DVD と同じ形の状態を返す）。
//! 開いたモデルと現在の位置をセッションに保持します（プロジェクト文書ではないので Undo には入りません）。

use super::always_enabled;
use crate::params::opt_str;
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{DiscKind, Link, Project};
use disccraft_vm::{
    BdDisc, BdLocation, BdState, BdVm, DiscModel, Key, Location, Registers, Vm, load_bdmv,
    load_video_ts,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::Path;

/// プレビューの状態。
pub(crate) struct Preview {
    /// 読み込んだディスク。
    model: Option<DiscModel>,
    /// 現在の位置。
    location: Location,
    /// VM のレジスタ（GPRM と SPRM）。キーを押す間も持ち越す（再生リストや上書きがタイトルをまたいで続く）。
    registers: Registers,
    /// Blu-ray のプロジェクトのプレビューか。
    bluray: bool,
    /// Blu-ray のタイトルごとの、ポップアップメニュー（リモートメニューがポップアップのメニューなら、そのページの
    /// PGCN。最初のページが先。ないタイトルは空）。モデルのタイトルの順（タイムライン、スライドショー、チャプター
    /// 再生リスト）。
    popups: Vec<Vec<u16>>,
    /// 再生中のタイトルの上に出ているポップアップメニュー（PGCN とハイライトしているボタン）。
    popup: Option<(u16, u8)>,
    /// ビルドした Blu-ray を開いているときのディスク（このときは上の DVD のモデルを使わない）。
    bd: Option<BdPreview>,
}

/// ビルドした Blu-ray のプレビュー: ナビゲーション、今の状態、プレイリストとプロジェクトの対応。
pub(crate) struct BdPreview {
    disc: BdDisc,
    state: BdState,
    /// プレイリストの並び（[`bd_layout`]）。
    layout: BdLayout,
    /// タイムラインごとのポップアップメニューのページ（メニューの順の 1 から。IG のページ n が n 番目）。
    popups: Vec<Vec<u16>>,
    /// ページを 1 つの IG にまとめたメニュー（M10.26）: まとまりごとのメニューの順（0 から、最初のページが先）。
    /// 最初のページのプレイリストで IG のページ n を見ているときは、n 番目のページのメニュー。
    groups: Vec<Vec<usize>>,
}

/// ビルドした Blu-ray のプレイリストの番号の並び（[`bd_layout`]）。
struct BdLayout {
    /// タイトルのプレイリストごとの、プレビューが知らせるタイトル（DVD と同じ番号: タイムライン、スライドショー、
    /// チャプター再生リストの順）と、再生する視聴制限の版（[`disccraft_project::Timeline::parental_variants`] の
    /// 番号。視聴制限のチャプターのあるタイムラインだけ）。
    titles: BTreeMap<u32, (u32, Option<usize>)>,
    /// タイムラインの数（そのタイトルが先頭に並ぶ）。
    timelines: u32,
    /// メニューのプレイリストの最初の番号（プロジェクトのメニューの順に続く）。
    menu_base: u32,
}

/// ビルドした Blu-ray のプレイリストの番号の並び（Blu-ray のビルドと同じ）: タイムラインのタイトル、視聴制限の
/// チャプターの代わりの映像の隠しタイトル（M6.548）、スライドショー、中身のあるチャプター再生リスト、視聴制限の版
/// （M6.548）、そしてメニューが 1 つずつ（M6.555: 再生リストはプレイリストを持たず、項目のタイトルのプレイリストを
/// 順に再生する）。M6.552: タイトルのプレイリストを、プレビューが知らせるタイトルの番号と版に読み替える表も（隠し
/// タイトルはタイトルとして数えない。版はそのタイムラインのタイトルの別の版）。
fn bd_layout(project: &Project) -> BdLayout {
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let timelines = count(project.timelines.len());
    let slideshows = count(project.slideshows.len());
    let versions = project.bd_parental_versions();
    // The hidden titles a build adds for parental chapters' alternative videos (after the timelines).
    let hidden = count(
        project
            .with_parental_clips()
            .map_or(0, |(_, clips)| clips.len()),
    );
    let mut titles = BTreeMap::new();
    let title_of = |index: usize| count(index).saturating_add(1);
    for (index, id) in project.timelines.keys().enumerate() {
        let parental = versions.iter().any(|(timeline, _)| timeline == id);
        titles.insert(title_of(index), (title_of(index), parental.then_some(0)));
    }
    let mut next = timelines.saturating_add(hidden).saturating_add(1);
    for index in 0..project.slideshows.len() {
        titles.insert(next, (timelines.saturating_add(title_of(index)), None));
        next = next.saturating_add(1);
    }
    for (index, list) in project.chapter_playlists.values().enumerate() {
        if list.chapters.is_empty() {
            continue;
        }
        let title = timelines
            .saturating_add(slideshows)
            .saturating_add(title_of(index));
        titles.insert(next, (title, None));
        next = next.saturating_add(1);
    }
    for (timeline, variant) in versions {
        if let Some(index) = project.timelines.keys().position(|id| *id == timeline) {
            titles.insert(next, (title_of(index), Some(variant)));
        }
        next = next.saturating_add(1);
    }
    BdLayout {
        titles,
        timelines,
        menu_base: next,
    }
}

/// `path` がビルドした Blu-ray（`BDMV` か、それを入れたフォルダー）か。
fn is_bdmv(path: &Path) -> bool {
    path.join("index.bdmv").is_file() || path.join("BDMV").join("index.bdmv").is_file()
}

impl BdPreview {
    fn open(
        path: &Path,
        project: &Project,
        (languages, parental): ((Option<[u8; 3]>, Option<[u8; 3]>), Option<u8>),
    ) -> Result<Self, String> {
        let disc = load_bdmv(path).map_err(|error| error.to_string())?;
        // M6.543: the player's parental level (1 to 8) as the age a Blu-ray reads in PSR13.
        let age = parental.and_then(disccraft_project::bd_parental_age);
        let state = BdVm::start_with_player(&disc, languages, age)
            .map_err(|error| error.to_string())?
            .state()
            .clone();
        let layout = bd_layout(project);
        let (popups, groups) = {
            let mut bluray = project.clone();
            bluray.settings.kind = DiscKind::BluRay;
            (
                popup_menus(&bluray),
                disccraft_build::menu_page_groups(&bluray),
            )
        };
        Ok(Self {
            disc,
            state,
            layout,
            popups,
            groups,
        })
    }

    fn press(&mut self, key: &str) -> Result<(), String> {
        let mut vm = BdVm::restore(&self.disc, self.state.clone());
        let result = match key {
            "up" => vm.press(Key::Up),
            "down" => vm.press(Key::Down),
            "left" => vm.press(Key::Left),
            "right" => vm.press(Key::Right),
            "enter" => vm.press(Key::Enter),
            "finish" => vm.finish(),
            "title" => vm.top_menu(),
            // Blu-ray のリモコンのポップアップ / メニューのキー: ポップアップのあるタイトルではそれを出し入れする。
            "menu" | "popup" => {
                vm.popup_key();
                Ok(())
            }
            "next" | "previous" => {
                let BdLocation::Playing { playlist, mark } = vm.state().location else {
                    return Err("not in a title".to_string());
                };
                let marks = self
                    .disc
                    .playlists
                    .get(&playlist)
                    .map_or(0, |found| found.marks);
                if key == "next"
                    && mark.saturating_add(1) >= marks
                    && playlist < self.layout.menu_base
                {
                    // M6.555: Next on a title's last chapter ends its playlist (a project playlist goes on with its
                    // next item, a title runs its end action), as in the DVD's preview.
                    vm.finish()
                } else if key == "next" {
                    vm.jump_mark(mark.saturating_add(1))
                } else {
                    vm.jump_mark(mark.saturating_sub(1))
                }
            }
            other => return Err(format!("unknown key \"{other}\"")),
        };
        result.map_err(|error| error.to_string())?;
        self.state = vm.state().clone();
        Ok(())
    }

    /// プレーヤーのチャプター番号（画面の再生が次のチャプターへ進んだときも）: 再生中のプレイリストのマーク。
    fn chapter(&mut self, chapter: u16) -> Result<(), String> {
        let mut vm = BdVm::restore(&self.disc, self.state.clone());
        vm.jump_mark(u32::from(chapter.saturating_sub(1)))
            .map_err(|error| error.to_string())?;
        self.state = vm.state().clone();
        Ok(())
    }

    fn streams(&mut self, audio: Option<u8>, subtitle: Option<Option<u8>>) {
        if let Some(track) = audio {
            self.state.audio = u16::from(track);
        }
        match subtitle {
            Some(Some(track)) => self.state.subtitle = (true, u16::from(track)),
            Some(None) => self.state.subtitle.0 = false,
            None => {}
        }
    }

    /// プレイリスト `playlist` が知らせるタイトルと視聴制限の版（[`BdLayout::titles`]。表にないもの（隠しタイトル）
    /// はその番号のまま）。
    fn title(&self, playlist: u32) -> (u32, Option<usize>) {
        self.layout
            .titles
            .get(&playlist)
            .copied()
            .unwrap_or((playlist, None))
    }

    /// DVD のプレビューと同じ形の状態。メニューのプレイリストはプロジェクトのメニューの順（`pgcn`）、
    /// タイトルはプレイリストの番号（タイムラインの順）とマーク（チャプター）。ボタンはページの中の順の 1 から。
    fn state(&self) -> Value {
        let vm = BdVm::restore(&self.disc, self.state.clone());
        let shown = vm.menu();
        // ページの絵のボタン（M10.26）は選べないので数えない。
        let button_number = |id: u16| -> u64 {
            shown
                .and_then(|(menu, view)| {
                    menu.pages
                        .get(usize::from(view.page))?
                        .buttons
                        .iter()
                        .filter(|button| button.id < disccraft_build::PAGE_PICTURE_BUTTON)
                        .position(|button| button.id == id)
                })
                .map_or(0, |index| {
                    u64::try_from(index).unwrap_or(0).saturating_add(1)
                })
        };
        let location = match self.state.location {
            BdLocation::Stopped => Value::String("stopped".to_string()),
            BdLocation::Playing { playlist, .. } if playlist >= self.layout.menu_base => {
                let index = playlist - self.layout.menu_base;
                // ページを 1 つの IG にまとめたメニューは、IG のページ n が n 番目のページ（0 ページ目は振り分け）。
                let page = shown.map_or(0, |(_, view)| usize::from(view.page));
                let pgcn = self
                    .groups
                    .iter()
                    .find(|group| {
                        group
                            .first()
                            .is_some_and(|first| u32::try_from(*first).ok() == Some(index))
                    })
                    .and_then(|group| group.get(page.checked_sub(1)?))
                    .and_then(|menu| u32::try_from(*menu).ok())
                    .unwrap_or(index);
                json!({
                    "menu": {
                        "pgcn": pgcn + 1,
                        "button": shown.map_or(0, |(_, view)| button_number(view.button)),
                    }
                })
            }
            BdLocation::Playing { playlist, mark } => json!({
                "title": { "title": self.title(playlist).0, "chapter": mark.saturating_add(1) }
            }),
        };
        // M6.552: the title a parental version is of, and the version.
        let (title, variant) = match self.state.location {
            BdLocation::Playing { playlist, .. } if playlist < self.layout.menu_base => {
                self.title(playlist)
            }
            _ => (0, None),
        };
        let popup_pages: &[u16] = if (1..=self.layout.timelines).contains(&title) {
            usize::try_from(title)
                .ok()
                .and_then(|title| title.checked_sub(1))
                .and_then(|index| self.popups.get(index))
                .map_or(&[], Vec::as_slice)
        } else {
            &[]
        };
        // ページのあるポップアップは、IG のページ n が n 番目のページのメニュー。
        let popup = match shown {
            Some((menu, view)) if menu.popup && view.open => popup_pages
                .get(usize::from(view.page))
                .or(popup_pages.first())
                .map_or(
                    Value::Null,
                    |pgcn| json!({ "pgcn": pgcn, "button": button_number(view.button) }),
                ),
            _ => Value::Null,
        };
        let (subtitle_on, subtitle_track) = self.state.subtitle;
        json!({
            "open": true,
            "bluray": true,
            "built": true,
            "location": location,
            "popup": popup,
            "popupAvailable": vm.has_popup() && !popup_pages.is_empty(),
            "audio": (self.state.audio > 0).then_some(self.state.audio),
            "variant": variant,
            "subtitle": { "on": subtitle_on, "track": (subtitle_track > 0).then_some(subtitle_track) },
            "menus": [],
            "titles": [],
        })
    }
}

impl Default for Preview {
    fn default() -> Self {
        Self {
            model: None,
            location: Location::Stopped,
            registers: Registers::default(),
            bluray: false,
            popups: Vec::new(),
            popup: None,
            bd: None,
        }
    }
}

/// Blu-ray のプロジェクトの、タイトルごとのポップアップメニュー（[`Preview::popups`]）。タイムラインの
/// リモートメニューが「Blu-ray のポップアップメニューに設定」したボタンのあるメニューなら、そのページの PGCN
/// （メニューの順。ページがまとめてポップアップになるメニューは全ページ、最初のページが先。そうでなければその
/// メニューだけ）。スライドショーとチャプター再生リストにはない（空）。DVD では空。
fn popup_menus(project: &Project) -> Vec<Vec<u16>> {
    if project.settings.kind != DiscKind::BluRay {
        return Vec::new();
    }
    let groups = disccraft_build::popup_page_groups(project);
    let pgcn = |index: usize| u16::try_from(index.saturating_add(1)).ok();
    project
        .timelines
        .values()
        .map(|timeline| match &timeline.menu_remote {
            Link::Menu { id, .. } => project
                .menus
                .get(id)
                .filter(|menu| menu.popup && !menu.buttons.is_empty())
                .and_then(|_| project.menus.keys().position(|key| key == id))
                .map_or_else(Vec::new, |index| {
                    groups
                        .iter()
                        .find(|group| group.contains(&index))
                        .map_or_else(|| vec![index], Clone::clone)
                        .into_iter()
                        .filter_map(pgcn)
                        .collect()
                }),
            _ => Vec::new(),
        })
        .collect()
}

impl Preview {
    /// ディスクを開いてファーストプレイを走らせる。`start` があればそこから（Encore の「ここからプレビューする」:
    /// メニューならそのメニュー、タイトルならそのチャプターの頭。レジスタは初期値）。
    pub(crate) fn open(
        &mut self,
        path: &Path,
        start: Option<Location>,
        parental: Option<u8>,
    ) -> Result<(), String> {
        let model = load_video_ts(path).map_err(|error| error.to_string())?;
        self.open_model(model, start, parental)
    }

    /// `model` を開いてファーストプレイ（または `start`）から走らせる。
    ///
    /// `parental`: the player's parental level (1 to 8), when it has one set. A disc of a higher level asks for
    /// the player's password, which the preview has no one to give: it stops, as a locked player does.
    pub(crate) fn open_model(
        &mut self,
        model: DiscModel,
        start: Option<Location>,
        parental: Option<u8>,
    ) -> Result<(), String> {
        let vm = match (start, parental) {
            (Some(location), _) => {
                Vm::restore(&model, location).map_err(|error| error.to_string())?
            }
            (None, Some(level)) => Vm::start_with_parental_level(&model, level, false)
                .map_err(|error| error.to_string())?,
            (None, None) => Vm::start(&model).map_err(|error| error.to_string())?,
        };

        self.location = *vm.location();
        self.registers = vm.registers();
        // The level stays the player's for the titles that follow (a title checks it again).
        if let (Some(level), Some(slot)) = (parental, self.registers.sprm.get_mut(13)) {
            *slot = u16::from(level);
        }
        self.model = Some(model);
        self.bluray = false;
        self.popups = Vec::new();
        self.popup = None;
        self.bd = None;
        Ok(())
    }

    /// ビルドした Blu-ray（`BDMV` か、それを入れたフォルダー）を開いてファーストプレイから。プレイリストと
    /// `project` のタイムライン・メニューの対応はビルドと同じ並びから。
    pub(crate) fn open_bd(
        &mut self,
        path: &Path,
        project: &Project,
        player: ((Option<[u8; 3]>, Option<[u8; 3]>), Option<u8>),
    ) -> Result<(), String> {
        let bd = BdPreview::open(path, project, player)?;
        self.model = None;
        self.location = Location::Stopped;
        self.registers = Registers::default();
        self.bluray = true;
        self.popups = Vec::new();
        self.popup = None;
        self.bd = Some(bd);
        Ok(())
    }

    /// 開いたのが Blu-ray のプロジェクトなら、そのタイトルのポップアップメニューを覚える。
    pub(crate) fn set_bluray(&mut self, project: &Project) {
        self.bluray = project.settings.kind == DiscKind::BluRay;
        self.popups = popup_menus(project);
    }

    /// 再生中のタイトルのポップアップメニューの最初のページの PGCN（Blu-ray で、そのタイトルにあるとき）。
    fn title_popup(&self) -> Option<u16> {
        self.title_popup_pages().first().copied()
    }

    /// 再生中のタイトルのポップアップメニューのページの PGCN（最初のページが先。ないときは空）。
    fn title_popup_pages(&self) -> &[u16] {
        let Location::Title { title, .. } = self.location else {
            return &[];
        };
        usize::from(title)
            .checked_sub(1)
            .and_then(|index| self.popups.get(index))
            .map_or(&[], Vec::as_slice)
    }

    /// ポップアップメニューが出ている間のキー: 十字キーはそのボタンを移り、決定はボタンのリンクへ（ポップアップは
    /// 閉じる。リンクがそのポップアップ自身かその別のページなら、出たままそのページへ）。メニュー / ポップアップの
    /// キーで閉じる。タイトルはその間も再生を続ける（Blu-ray のポップアップメニュー）。ほかのキーはポップアップを
    /// 閉じてから。
    fn press_in_popup(&mut self, key: &str, pgcn: u16, button: u8) -> Result<bool, String> {
        let pressed = match key {
            "menu" | "popup" => {
                self.popup = None;
                return Ok(true);
            }
            "up" => Key::Up,
            "down" => Key::Down,
            "left" => Key::Left,
            "right" => Key::Right,
            "enter" => Key::Enter,
            // チャプターのキーは、ポップアップを出したままタイトルの中を動く。
            "next" | "previous" => return Ok(false),
            _ => {
                self.popup = None;
                return Ok(false);
            }
        };
        let pages = self.title_popup_pages().to_vec();
        let model = self.model.as_ref().ok_or("preview is not open")?;
        let mut vm =
            Vm::restore_with_registers(model, Location::Menu { pgcn, button }, self.registers)
                .map_err(|error| error.to_string())?;
        vm.press(pressed).map_err(|error| error.to_string())?;
        match *vm.location() {
            // ハイライトが動いた（または決定しても同じポップアップに残った、そのポップアップの別のページへ移った）。
            Location::Menu {
                pgcn: shown,
                button,
            } if shown == pgcn || pages.contains(&shown) => self.popup = Some((shown, button)),
            location => {
                self.popup = None;
                self.location = location;
                self.registers = vm.registers();
            }
        }
        Ok(true)
    }

    /// 読み込んだモデルを直接使う（テスト用）。
    #[cfg(test)]
    pub(crate) fn with_model(model: DiscModel) -> Result<Self, String> {
        let vm = Vm::start(&model).map_err(|error| error.to_string())?;

        Ok(Self {
            location: *vm.location(),
            registers: vm.registers(),
            model: Some(model),
            ..Self::default()
        })
    }

    /// リモコンキーを送る。`finish` はタイトルを終わらせる。Blu-ray では、ポップアップメニューのあるタイトルの
    /// 再生中に `menu`（`popup`）でそのポップアップを映像の上に出し、出ている間はキーがポップアップに行く。
    pub(crate) fn press(&mut self, key: &str) -> Result<(), String> {
        if let Some(bd) = self.bd.as_mut() {
            return bd.press(key);
        }
        if self.model.is_none() {
            return Err("preview is not open".to_string());
        }
        if let Some((pgcn, button)) = self.popup
            && self.press_in_popup(key, pgcn, button)?
        {
            return Ok(());
        }
        if matches!(key, "menu" | "popup")
            && let Some(pgcn) = self.title_popup()
        {
            self.popup = Some((pgcn, 1));
            return Ok(());
        }
        // ポップアップのないところのポップアップキー: プレーヤーは何もしない。
        if key == "popup" {
            return Ok(());
        }
        let model = self.model.as_ref().ok_or("preview is not open")?;
        let mut vm = Vm::restore_with_registers(model, self.location, self.registers)
            .map_err(|error| error.to_string())?;

        match key {
            "up" => vm.press(Key::Up),
            "down" => vm.press(Key::Down),
            "left" => vm.press(Key::Left),
            "right" => vm.press(Key::Right),
            "enter" => vm.press(Key::Enter),
            "finish" => match vm.location() {
                Location::Menu { .. } => vm.time_out_menu(),
                _ => vm.finish_title(),
            },
            "menu" => vm.menu_key(),
            "title" => vm.title_key(),
            // The player's chapter keys (|◀◀ ▶▶|): the chapter before or after, within the title.
            // Next on the last chapter ends the title (its end action runs), as in Encore's preview.
            "next" => vm.next_chapter(),
            "previous" => {
                let Location::Title { chapter, .. } = *vm.location() else {
                    return Err("not in a title".to_string());
                };
                vm.jump_chapter(chapter.saturating_sub(1))
            }
            other => return Err(format!("unknown key \"{other}\"")),
        }
        .map_err(|error| error.to_string())?;

        self.location = *vm.location();
        self.registers = vm.registers();
        Ok(())
    }

    /// プレーヤーのチャプター番号の入力: 再生中のタイトルの `chapter` へ（端を越えたら最初か最後）。
    pub(crate) fn chapter(&mut self, chapter: u16) -> Result<(), String> {
        if let Some(bd) = self.bd.as_mut() {
            return bd.chapter(chapter);
        }
        let model = self.model.as_ref().ok_or("preview is not open")?;
        let mut vm = Vm::restore_with_registers(model, self.location, self.registers)
            .map_err(|error| error.to_string())?;
        vm.jump_chapter(chapter)
            .map_err(|error| error.to_string())?;
        self.location = *vm.location();
        self.registers = vm.registers();
        Ok(())
    }

    /// プレーヤーのオーディオ / 字幕のキー: オーディオトラック（1〜8）と字幕トラック（1〜32、`Some(None)` は消す）。
    pub(crate) fn streams(
        &mut self,
        audio: Option<u8>,
        subtitle: Option<Option<u8>>,
    ) -> Result<(), String> {
        if let Some(bd) = self.bd.as_mut() {
            bd.streams(audio, subtitle);
            return Ok(());
        }
        let model = self.model.as_ref().ok_or("preview is not open")?;
        let mut vm = Vm::restore_with_registers(model, self.location, self.registers)
            .map_err(|error| error.to_string())?;
        if let Some(track) = audio {
            vm.set_audio_track(track)
                .map_err(|error| error.to_string())?;
        }
        if let Some(track) = subtitle {
            vm.set_subtitle_track(track)
                .map_err(|error| error.to_string())?;
        }
        self.registers = vm.registers();
        Ok(())
    }

    /// 現在の状態を JSON にする。
    pub(crate) fn state(&self) -> Value {
        if let Some(bd) = &self.bd {
            return bd.state();
        }
        let Some(model) = self.model.as_ref() else {
            return json!({ "open": false });
        };

        let menus: Vec<Value> = model
            .vmgm
            .iter()
            .enumerate()
            .map(|(index, menu)| {
                json!({
                    "pgcn": index.saturating_add(1),
                    "buttons": menu.buttons.len(),
                    "hasCells": menu.has_cells,
                })
            })
            .collect();

        let (subtitle_on, subtitle_track) = {
            let vm = Vm::restore_with_registers(model, self.location, self.registers);
            vm.as_ref().map(Vm::subtitle_track).unwrap_or((false, None))
        };
        let audio_track = Vm::restore_with_registers(model, self.location, self.registers)
            .ok()
            .and_then(|vm| vm.audio_track());
        // M13.19: the version of a title with parental chapters the player plays (0: every chapter as it is).
        let variant = Vm::restore_with_registers(model, self.location, self.registers)
            .ok()
            .and_then(|vm| vm.title_variant());
        let titles: Vec<Value> = model
            .titles
            .iter()
            .enumerate()
            .map(|(index, title)| {
                json!({ "title": index.saturating_add(1), "chapters": title.chapters })
            })
            .collect();

        let mut map = Map::new();
        map.insert("open".to_string(), Value::Bool(true));
        map.insert("location".to_string(), location_value(self.location));
        map.insert("bluray".to_string(), Value::Bool(self.bluray));
        map.insert(
            "popup".to_string(),
            self.popup.map_or(
                Value::Null,
                |(pgcn, button)| json!({ "pgcn": pgcn, "button": button }),
            ),
        );
        map.insert(
            "popupAvailable".to_string(),
            Value::Bool(self.title_popup().is_some()),
        );
        map.insert("audio".to_string(), json!(audio_track));
        map.insert("variant".to_string(), json!(variant));
        map.insert(
            "subtitle".to_string(),
            json!({ "on": subtitle_on, "track": subtitle_track }),
        );
        map.insert("menus".to_string(), Value::Array(menus));
        map.insert("titles".to_string(), Value::Array(titles));
        Value::Object(map)
    }

    /// Sets the player's country (SPRM12, ISO 3166 alpha-2; M13.19).
    pub(crate) fn set_country(&mut self, country: [u8; 2]) {
        if let Some(slot) = self.registers.sprm.get_mut(12) {
            *slot = u16::from_be_bytes(country);
        }
    }

    /// 現在の位置（テストと将来の制御チャネル用）。
    #[cfg(test)]
    pub(crate) fn location(&self) -> Location {
        self.location
    }

    pub(crate) fn is_open(&self) -> bool {
        self.model.is_some() || self.bd.is_some()
    }

    pub(crate) fn close(&mut self) {
        self.model = None;
        self.location = Location::Stopped;
        self.popup = None;
        self.bd = None;
    }
}

fn location_value(location: Location) -> Value {
    match location {
        Location::Stopped => Value::String("stopped".to_string()),
        Location::Menu { pgcn, button } => json!({ "menu": { "pgcn": pgcn, "button": button } }),
        Location::Title { title, chapter } => {
            json!({ "title": { "title": title, "chapter": chapter } })
        }
    }
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "preview.open",
        label: "Open Preview",
        menu: &["Preview"],
        shortcut: None,
        params: r#"{"path"?: "a VIDEO_TS folder (without it, from the project without building)", "menu"?: the VMGM's PGCN, "button"?: n (with menu, the button selected; from 1), "title"?: n, "chapter"?: n, "parentalLevel"?: 1..8 (the player's parental level; a built Blu-ray reads it as an age in PSR13, M6.543), "audioLanguage"?: "en" and so on (a built Blu-ray: the player's audio language), "subtitleLanguage"?: the same for subtitles, "country"?: "JP" and so on (the player's country, ISO 3166, which picks a DVD parental block's version; default US)}"#,
        enabled: always_enabled,
        run: run_open,
        journal: false,
    },
    CommandSpec {
        id: "preview.key",
        label: "Remote Control Key",
        menu: &["Preview"],
        shortcut: None,
        params: r#"{"key": "up"|"down"|"left"|"right"|"enter"|"finish"|"menu"|"title"|"next"|"previous"}"#,
        enabled: enabled_open,
        run: run_key,
        journal: false,
    },
    CommandSpec {
        id: "preview.chapter",
        label: "Go to Chapter",
        menu: &["Preview"],
        shortcut: None,
        params: r#"{"chapter": n}"#,
        enabled: enabled_open,
        run: run_chapter,
        journal: false,
    },
    CommandSpec {
        id: "preview.streams",
        label: "Choose Audio and Subtitles",
        menu: &["Preview"],
        shortcut: None,
        params: r#"{"audio"?: 1..8, "subtitle"?: 1..32 | "off"}"#,
        enabled: enabled_open,
        run: run_streams,
        journal: false,
    },
    CommandSpec {
        id: "preview.state",
        label: "Preview State",
        menu: &["Preview"],
        shortcut: None,
        params: "{}",
        enabled: always_enabled,
        run: run_state,
        journal: false,
    },
    CommandSpec {
        id: "preview.close",
        label: "Close Preview",
        menu: &["Preview"],
        shortcut: None,
        params: "{}",
        enabled: always_enabled,
        run: run_close,
        journal: false,
    },
];

fn enabled_open(session: &Session) -> Result<(), String> {
    if session.preview.is_open() {
        Ok(())
    } else {
        Err("preview is not open".to_string())
    }
}

fn run_open(session: &mut Session, params: &Value) -> CommandResult {
    let path = opt_str(params, "path")?.filter(|path| !path.trim().is_empty());
    let small = |name: &str| -> Result<Option<u16>, String> {
        match params.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_u64()
                .and_then(|n| u16::try_from(n).ok())
                .filter(|n| *n > 0)
                .map(Some)
                .ok_or_else(|| format!("parameter \"{name}\" must be a positive number")),
        }
    };
    // M13.19: the player's country (SPRM12), with which a DVD's parental block looks its level's mask up; a player
    // always has one, so the preview's is the United States unless it is given.
    let country: [u8; 2] = match opt_str(params, "country")?.filter(|code| !code.is_empty()) {
        None => *b"US",
        Some(code) => code
            .to_ascii_uppercase()
            .as_bytes()
            .try_into()
            .ok()
            .filter(|code: &[u8; 2]| code.iter().all(u8::is_ascii_uppercase))
            .ok_or_else(|| "parameter \"country\" must be a two-letter code".to_string())?,
    };
    let parental = match params.get("parentalLevel") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .and_then(|level| u8::try_from(level).ok())
                .filter(|level| (1..=8).contains(level))
                .ok_or_else(|| "parameter \"parentalLevel\" must be 1 to 8".to_string())?,
        ),
    };
    let start = if let Some(pgcn) = small("menu")? {
        // Encore's ここからプレビューする on a button (M6.338): the menu with that button selected.
        let button = small("button")?
            .map(|button| {
                u8::try_from(button).map_err(|_| "parameter \"button\" is too large".to_string())
            })
            .transpose()?
            .unwrap_or(1);
        Some(Location::Menu { pgcn, button })
    } else if let Some(title) = small("title")? {
        let title =
            u8::try_from(title).map_err(|_| "parameter \"title\" is too large".to_string())?;
        Some(Location::Title {
            title,
            chapter: small("chapter")?.unwrap_or(1),
        })
    } else {
        None
    };
    match path {
        // ビルドした Blu-ray: HDMV の解釈器で。
        Some(path) if is_bdmv(Path::new(&path)) => {
            // The player's languages (ISO 639-1, Preferences > Preview; M6.369) choose the streams while the
            // disc chooses none.
            let language = |name: &str| -> Result<Option<[u8; 3]>, String> {
                match opt_str(params, name)?.filter(|code| !code.is_empty()) {
                    None => Ok(None),
                    Some(code) => {
                        let two: [u8; 2] = code
                            .to_ascii_lowercase()
                            .as_bytes()
                            .try_into()
                            .map_err(|_| {
                                format!("parameter \"{name}\" must be a two-letter code")
                            })?;
                        Ok(Some(disccraft_bdmv::iso639_2(Some(two))))
                    }
                }
            };
            let languages = (language("audioLanguage")?, language("subtitleLanguage")?);
            session
                .preview
                .open_bd(Path::new(&path), &session.project, (languages, parental))?;
        }
        Some(path) => {
            session.preview.open(Path::new(&path), start, parental)?;
            session.preview.set_country(country);
        }
        // Encore のプロジェクトプレビュー: ビルドせずに、ビルドが書くのと同じナビゲーションで（ボタンの
        // トランジションも隠しタイトルに広げて）。
        None => {
            let project = session
                .project
                .with_transitions_expanded()
                .map_err(|error| error.to_string())?;
            let model =
                disccraft_dvd::preview_model(&project).map_err(|error| error.to_string())?;
            session.preview.open_model(model, start, parental)?;
            session.preview.set_country(country);
            session.preview.set_bluray(&project);
        }
    }
    Ok(session.preview.state())
}

fn run_key(session: &mut Session, params: &Value) -> CommandResult {
    let key = opt_str(params, "key")?.unwrap_or_else(|| "enter".to_string());
    session.preview.press(&key)?;
    Ok(session.preview.state())
}

fn run_chapter(session: &mut Session, params: &Value) -> CommandResult {
    let chapter = params
        .get("chapter")
        .and_then(Value::as_u64)
        .and_then(|n| u16::try_from(n).ok())
        .ok_or_else(|| "parameter \"chapter\" must be a chapter number".to_string())?;
    session.preview.chapter(chapter)?;
    Ok(session.preview.state())
}

fn run_streams(session: &mut Session, params: &Value) -> CommandResult {
    let audio = match params.get("audio") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or_else(|| "parameter \"audio\" must be 1 to 8".to_string())?,
        ),
    };
    let subtitle = match params.get("subtitle") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text == "off" => Some(None),
        Some(value) => Some(Some(
            value
                .as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or_else(|| "parameter \"subtitle\" must be 1 to 32 or \"off\"".to_string())?,
        )),
    };
    session.preview.streams(audio, subtitle)?;
    Ok(session.preview.state())
}

fn run_state(session: &mut Session, _params: &Value) -> CommandResult {
    Ok(session.preview.state())
}

fn run_close(session: &mut Session, _params: &Value) -> CommandResult {
    session.preview.close();
    Ok(json!({ "open": false }))
}

#[cfg(test)]
mod tests {
    use super::Preview;
    use crate::Session;
    use disccraft_vm::{ButtonModel, DiscModel, Location, MenuPgcModel, TitleModel};
    use serde_json::{Value, json};
    use std::error::Error;

    /// ファーストプレイ → メニュー 1（ボタン 2 つ）→ タイトル 1（2 チャプター）→ メニューへ戻る。
    fn sample_disc() -> DiscModel {
        DiscModel {
            parental: None,
            title_menu: None,
            first_play: vec![[0x30, 0x06, 0x00, 0x01, 0x00, 0xC0, 0x00, 0x00]],
            vmgm: vec![MenuPgcModel {
                prohibited_ops: 0,
                pre: Vec::new(),
                post: Vec::new(),
                has_cells: true,
                buttons: vec![
                    ButtonModel {
                        command: [0x30, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00],
                        up: 2,
                        down: 2,
                        left: 1,
                        right: 1,
                        auto_action: false,
                    },
                    ButtonModel {
                        command: [0x30, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00],
                        up: 1,
                        down: 1,
                        left: 2,
                        right: 2,
                        auto_action: false,
                    },
                ],
                default_button: 0,
            }],
            titles: vec![TitleModel {
                prohibited_ops: 0,
                pre: Vec::new(),
                post: vec![[0x30, 0x08, 0x00, 0x01, 0x00, 0xC0, 0x00, 0x00]],
                chapters: 2,
                // MENU: JumpSS VMGM pgcn 1.
                menu_remote: vec![[0x30, 0x06, 0x00, 0x01, 0x00, 0xC0, 0x00, 0x00]],
            }],
        }
    }

    fn location_of(state: &Value) -> Option<Location> {
        let location = state.get("location")?;

        if let Some(menu) = location.get("menu") {
            return Some(Location::Menu {
                pgcn: menu.get("pgcn")?.as_u64()? as u16,
                button: menu.get("button")?.as_u64()? as u8,
            });
        }

        if let Some(title) = location.get("title") {
            return Some(Location::Title {
                title: title.get("title")?.as_u64()? as u8,
                chapter: title.get("chapter")?.as_u64()? as u16,
            });
        }

        None
    }

    /// M6.533: during a title, MENU shows the root menu; its Resume button (RSM), or MENU again, goes back to where
    /// the title was (the preview kept only the place and the registers between keys, and lost that point).
    #[test]
    fn resume_goes_back_to_where_menu_was_pressed() -> Result<(), Box<dyn Error>> {
        let mut disc = sample_disc();
        if let Some(menu) = disc.vmgm.first_mut()
            && let Some(button) = menu.buttons.first_mut()
        {
            button.command = [0x20, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10];
        }
        let mut preview = Preview::with_model(disc)?;
        // The menu's second button plays the title; Next goes to its chapter 2.
        preview.press("down")?;
        preview.press("enter")?;
        preview.press("next")?;
        let playing = Location::Title {
            title: 1,
            chapter: 2,
        };
        assert_eq!(location_of(&preview.state()), Some(playing));
        preview.press("menu")?;
        // The menu opens on its first button, which resumes.
        assert_eq!(
            location_of(&preview.state()),
            Some(Location::Menu { pgcn: 1, button: 1 })
        );
        preview.press("enter")?;
        assert_eq!(location_of(&preview.state()), Some(playing));

        // MENU, then MENU again in the menu: back to the title too.
        preview.press("menu")?;
        preview.press("menu")?;
        assert_eq!(location_of(&preview.state()), Some(playing));
        Ok(())
    }

    #[test]
    fn registers_carry_over_between_keys() -> Result<(), Box<dyn Error>> {
        // Title 1 sets GPRM7 = 1 and plays title 2. Title 2 stops when GPRM7 != GPRM5 (= 0), else goes to the menu.
        let mut disc = sample_disc();
        disc.first_play = vec![[0x30, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00]];
        disc.titles = vec![
            TitleModel {
                prohibited_ops: 0,
                pre: Vec::new(),
                post: vec![
                    [0x71, 0x00, 0x00, 0x07, 0x00, 0x01, 0x00, 0x00],
                    [0x30, 0x02, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00],
                ],
                chapters: 1,
                menu_remote: Vec::new(),
            },
            TitleModel {
                prohibited_ops: 0,
                pre: Vec::new(),
                post: vec![
                    [0x71, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00],
                    // if (GPRM7 != GPRM5) Exit
                    [0x30, 0x31, 0x00, 0x00, 0x00, 0x00, 0x07, 0x05],
                    [0x30, 0x08, 0x00, 0x01, 0x00, 0xC0, 0x00, 0x00],
                ],
                chapters: 1,
                menu_remote: Vec::new(),
            },
        ];
        let mut preview = Preview::with_model(disc)?;
        preview.press("finish")?;
        assert_eq!(
            location_of(&preview.state()),
            Some(Location::Title {
                title: 2,
                chapter: 1
            })
        );
        preview.press("finish")?;
        assert_eq!(location_of(&preview.state()), None, "stopped, not the menu");
        Ok(())
    }

    #[test]
    fn preview_tracks_the_remote() -> Result<(), Box<dyn Error>> {
        let mut preview = Preview::with_model(sample_disc())?;
        let state = preview.state();

        assert_eq!(
            location_of(&state),
            Some(Location::Menu { pgcn: 1, button: 1 })
        );
        assert_eq!(
            state.get("menus").and_then(Value::as_array).map(Vec::len),
            Some(1)
        );
        assert_eq!(
            state.get("titles").and_then(Value::as_array).map(Vec::len),
            Some(1)
        );

        preview.press("down")?;
        assert_eq!(
            location_of(&preview.state()),
            Some(Location::Menu { pgcn: 1, button: 2 })
        );

        preview.press("enter")?;
        assert_eq!(
            location_of(&preview.state()),
            Some(Location::Title {
                title: 1,
                chapter: 1
            })
        );

        // メニュー外では矢印キーが使えない。
        assert!(preview.press("up").is_err());

        // 再生中のメニューボタンはリモートメニューへ。メニューに戻ったらタイトルへ入り直す。
        preview.press("menu")?;
        assert!(matches!(
            location_of(&preview.state()),
            Some(Location::Menu { pgcn: 1, .. })
        ));
        preview.press("enter")?;

        preview.press("finish")?;
        assert_eq!(
            location_of(&preview.state()),
            Some(Location::Menu { pgcn: 1, button: 1 })
        );

        preview.close();
        assert_eq!(
            preview.state().get("open").and_then(Value::as_bool),
            Some(false)
        );

        Ok(())
    }

    #[test]
    fn preview_commands_require_an_open_disc() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();

        assert_eq!(
            session.preview.state().get("open").and_then(Value::as_bool),
            Some(false)
        );

        let error = session
            .execute("preview.key", &json!({ "key": "up" }))
            .err()
            .ok_or("expected a closed-preview error")?;
        assert!(error.contains("preview is not open"), "{error}");

        Ok(())
    }

    /// Encore のプロジェクトプレビュー: ビルドせずにプロジェクトから開き、メニューのボタンでタイトルへ。
    #[test]
    fn the_project_previews_without_a_build() -> Result<(), Box<dyn Error>> {
        use disccraft_project::Link;
        let dir =
            std::env::temp_dir().join(format!("disccraft-preview-project-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        // The preview reads no media: an empty file stands for the video.
        let video = dir.join("movie.m2v");
        std::fs::write(&video, b"")?;
        let mut session = Session::new();
        let asset = session.project.add_asset(&video)?;
        let timeline = session.project.add_timeline(asset, None)?;
        let menu = session.project.add_menu(Some("Top".to_owned()))?;
        session.project.add_button(
            menu,
            "Play".to_owned(),
            None,
            Link::Timeline {
                id: timeline,
                chapter: 0,
            },
        )?;
        session.project.settings.first_play = Link::Menu {
            id: menu,
            button: None,
        };

        // M6.338: from the menu with one of its buttons selected (Encore's ここからプレビューする on a button).
        session
            .project
            .add_button(menu, "Stop".to_owned(), None, Link::Stop)?;
        session.execute("preview.open", &json!({ "menu": 1, "button": 2 }))?;
        assert_eq!(
            session.preview.location(),
            Location::Menu { pgcn: 1, button: 2 }
        );
        let state = session.execute("preview.open", &json!({}))?;
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(state.get("open").and_then(Value::as_bool), Some(true));
        assert_eq!(
            session.preview.location(),
            Location::Menu { pgcn: 1, button: 1 }
        );
        session.execute("preview.key", &json!({ "key": "enter" }))?;
        assert_eq!(
            session.preview.location(),
            Location::Title {
                title: 1,
                chapter: 1
            }
        );
        // The previous key and the chapter number stay within the title's one chapter (next on the last chapter
        // ends the title, as in Encore's preview); the stream keys set SPRM1 / SPRM2.
        session.execute("preview.key", &json!({ "key": "previous" }))?;
        session.execute("preview.chapter", &json!({ "chapter": 9 }))?;
        assert_eq!(
            session.preview.location(),
            Location::Title {
                title: 1,
                chapter: 1
            }
        );
        let state = session.execute("preview.streams", &json!({ "audio": 2, "subtitle": 3 }))?;
        assert_eq!(state.get("audio").and_then(Value::as_u64), Some(2));
        assert_eq!(
            state.pointer("/subtitle/track").and_then(Value::as_u64),
            Some(3)
        );
        assert!(
            session
                .execute("preview.streams", &json!({ "audio": 0 }))
                .is_err()
        );
        Ok(())
    }

    /// M6.257: the preview's player can have a parental level; a disc of a higher level does not play on it.
    #[test]
    fn the_previews_player_has_a_parental_level() -> Result<(), Box<dyn Error>> {
        use disccraft_project::Link;
        let dir =
            std::env::temp_dir().join(format!("disccraft-preview-parental-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let video = dir.join("movie.m2v");
        std::fs::write(&video, b"")?;
        let mut session = Session::new();
        let asset = session.project.add_asset(&video)?;
        let timeline = session.project.add_timeline(asset, None)?;
        session.project.settings.first_play = Link::Timeline {
            id: timeline,
            chapter: 0,
        };
        session.project.settings.parental_level = 6;
        let playing = Location::Title {
            title: 1,
            chapter: 1,
        };
        // A player with no level set, and one set as high as the disc: the title plays.
        session.execute("preview.open", &json!({}))?;
        assert_eq!(session.preview.location(), playing);
        session.execute("preview.open", &json!({ "parentalLevel": 6 }))?;
        assert_eq!(session.preview.location(), playing);
        // One set lower: the disc asks for the password, there is no one to give it, and it stops.
        session.execute("preview.open", &json!({ "parentalLevel": 5 }))?;
        assert_eq!(session.preview.location(), Location::Stopped);
        // A disc without a level plays on any player.
        session.project.settings.parental_level = 0;
        session.execute("preview.open", &json!({ "parentalLevel": 1 }))?;
        assert_eq!(session.preview.location(), playing);
        assert!(
            session
                .execute("preview.open", &json!({ "parentalLevel": 9 }))
                .is_err()
        );
        // M6.363: the title asks 4 of its own and plays a second title in its place when it is not raised.
        let other = session.project.add_timeline(asset, None)?;
        session.execute(
            "timeline.setParentalLevel",
            &json!({ "timeline": timeline.0, "level": 4 }),
        )?;
        session.execute(
            "timeline.setParentalAlternative",
            &json!({ "timeline": timeline.0, "target": { "timeline": other.0 } }),
        )?;
        session.execute("preview.open", &json!({ "parentalLevel": 3 }))?;
        assert_eq!(
            session.preview.location(),
            Location::Title {
                title: 2,
                chapter: 1
            }
        );
        session.execute("preview.open", &json!({ "parentalLevel": 4 }))?;
        assert_eq!(session.preview.location(), playing);
        // A title cannot stand in for itself; levels past 8 are refused.
        assert!(
            session
                .execute(
                    "timeline.setParentalAlternative",
                    &json!({ "timeline": timeline.0, "target": { "timeline": timeline.0 } }),
                )
                .is_err()
        );
        assert!(
            session
                .execute(
                    "timeline.setParentalLevel",
                    &json!({ "timeline": timeline.0, "level": 9 }),
                )
                .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// M13.19: a DVD title with a chapter for level 5 that has a parental alternative: the state says which version
    /// of it the player plays, as the built disc's parental block chooses (M13.17): every chapter as it is on a player
    /// with no level, or one set to 5 or more; the alternative's version on one set lower.
    #[test]
    fn the_preview_says_which_parental_version_plays() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{Chapter, Link, ParentalChapter, Tick};
        let dir = std::env::temp_dir().join(format!(
            "disccraft-preview-parental-version-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        let (video, cut) = (dir.join("movie.m2v"), dir.join("cut.m2v"));
        std::fs::write(&video, b"")?;
        std::fs::write(&cut, b"")?;
        let mut session = Session::new();
        session.project.settings.kind = disccraft_project::DiscKind::Dvd;
        let asset = session.project.add_asset(&video)?;
        let alternative = session.project.add_asset(&cut)?;
        let timeline = session.project.add_timeline(asset, None)?;
        session.project.settings.first_play = Link::Timeline {
            id: timeline,
            chapter: 0,
        };
        let edited = session
            .project
            .timeline_mut(timeline)
            .ok_or("no timeline")?;
        edited.chapters.push(Chapter {
            // A second in (ticks of a 29.97 frame times 30).
            time: Tick(30 * 8_475_667_200),
            name: String::new(),
            poster: None,
        });
        edited.parental_chapters = vec![ParentalChapter {
            chapter: 1,
            level: 5,
            alternative,
        }];
        for (level, version) in [
            (None, 0),
            (Some(8), 0),
            (Some(5), 0),
            (Some(4), 1),
            (Some(1), 1),
        ] {
            let params = level.map_or_else(|| json!({}), |level| json!({ "parentalLevel": level }));
            let state = session.execute("preview.open", &params)?;
            assert_eq!(
                state.get("variant"),
                Some(&json!(version)),
                "level {level:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// A Blu-ray project previews too; a timeline whose remote menu is a pop-up menu shows it over the title with
    /// the menu key, its buttons take the arrows and Enter (here to the second chapter), and the key closes it.
    #[test]
    fn a_bluray_project_previews_with_its_popup_menu() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{DiscKind, Link, Tick};
        let dir =
            std::env::temp_dir().join(format!("disccraft-preview-popup-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let video = dir.join("movie.m2v");
        std::fs::write(&video, b"")?;
        let mut session = Session::new();
        session.project.settings.kind = DiscKind::BluRay;
        let asset = session.project.add_asset(&video)?;
        let timeline = session.project.add_timeline(asset, None)?;
        session
            .project
            .add_chapter(timeline, Tick(5 * 254_016_000_000), None)?;
        let popup = session.project.add_menu(Some("Scenes".to_owned()))?;
        if let Some(menu) = session.project.menus.get_mut(&popup) {
            menu.buttons.clear();
            menu.popup = true;
        }
        for chapter in [0, 1] {
            session.project.add_button(
                popup,
                format!("Scene {}", chapter + 1),
                None,
                Link::Timeline {
                    id: timeline,
                    chapter,
                },
            )?;
        }
        if let Some(found) = session.project.timeline_mut(timeline) {
            found.menu_remote = Link::Menu {
                id: popup,
                button: None,
            };
        }
        session.project.settings.first_play = Link::Timeline {
            id: timeline,
            chapter: 0,
        };

        let state = session.execute("preview.open", &json!({}))?;
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(state.get("bluray").and_then(Value::as_bool), Some(true));
        assert_eq!(
            state.get("popupAvailable").and_then(Value::as_bool),
            Some(true)
        );
        let title = Location::Title {
            title: 1,
            chapter: 1,
        };
        assert_eq!(session.preview.location(), title);

        // The menu key shows the pop-up over the playing title.
        let state = session.execute("preview.key", &json!({ "key": "menu" }))?;
        assert_eq!(
            state.pointer("/popup/pgcn").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            state.pointer("/popup/button").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(session.preview.location(), title);
        // The arrows move within it; the title goes on.
        let state = session.execute("preview.key", &json!({ "key": "down" }))?;
        let button = state.pointer("/popup/button").and_then(Value::as_u64);
        assert!(button.is_some());
        assert_eq!(session.preview.location(), title);
        // The key again closes it.
        let state = session.execute("preview.key", &json!({ "key": "popup" }))?;
        assert_eq!(state.get("popup"), Some(&Value::Null));
        // Enter on the second scene plays its chapter and closes the pop-up.
        session.execute("preview.key", &json!({ "key": "popup" }))?;
        session.preview.popup = Some((1, 2));
        let state = session.execute("preview.key", &json!({ "key": "enter" }))?;
        assert_eq!(state.get("popup"), Some(&Value::Null));
        assert_eq!(
            session.preview.location(),
            Location::Title {
                title: 1,
                chapter: 2
            }
        );
        Ok(())
    }

    /// GitHub issue #13: a pop-up menu with pages stays over the playing title when its button goes to its other
    /// page (that page's menu is drawn), and back.
    #[test]
    fn a_popups_pages_turn_over_the_title_in_the_preview() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{DiscKind, Link, Tick};
        let dir = std::env::temp_dir().join(format!(
            "disccraft-preview-popup-pages-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        let video = dir.join("movie.m2v");
        std::fs::write(&video, b"")?;
        let mut session = Session::new();
        session.project.settings.kind = DiscKind::BluRay;
        let asset = session.project.add_asset(&video)?;
        let timeline = session.project.add_timeline(asset, None)?;
        session
            .project
            .add_chapter(timeline, Tick(5 * 254_016_000_000), None)?;
        let popup = session.project.add_menu(Some("Scenes".to_owned()))?;
        if let Some(menu) = session.project.menus.get_mut(&popup) {
            menu.buttons.clear();
            menu.popup = true;
        }
        let page = session.project.add_menu_page(popup)?;
        session.project.add_button(
            popup,
            "Scene 1".to_owned(),
            None,
            Link::Timeline {
                id: timeline,
                chapter: 0,
            },
        )?;
        session.project.add_button(
            popup,
            "More".to_owned(),
            None,
            Link::Menu {
                id: page,
                button: Some(0),
            },
        )?;
        session.project.add_button(
            page,
            "Back".to_owned(),
            None,
            Link::Menu {
                id: popup,
                button: Some(1),
            },
        )?;
        session.project.add_button(
            page,
            "Scene 2".to_owned(),
            None,
            Link::Timeline {
                id: timeline,
                chapter: 1,
            },
        )?;
        if let Some(found) = session.project.timeline_mut(timeline) {
            found.menu_remote = Link::Menu {
                id: popup,
                button: None,
            };
        }
        session.project.settings.first_play = Link::Timeline {
            id: timeline,
            chapter: 0,
        };

        session.execute("preview.open", &json!({}))?;
        let _ = std::fs::remove_dir_all(&dir);
        let title = Location::Title {
            title: 1,
            chapter: 1,
        };
        session.execute("preview.key", &json!({ "key": "menu" }))?;
        // More: the second page (the second menu), its first button, over the same title.
        session.preview.popup = Some((1, 2));
        let state = session.execute("preview.key", &json!({ "key": "enter" }))?;
        assert_eq!(
            state.pointer("/popup/pgcn").and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(
            state.pointer("/popup/button").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(session.preview.location(), title);
        // Back: the first page's More.
        let state = session.execute("preview.key", &json!({ "key": "enter" }))?;
        assert_eq!(
            state.pointer("/popup/pgcn").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            state.pointer("/popup/button").and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(session.preview.location(), title);
        Ok(())
    }

    /// M6.552: a built Blu-ray's playlists read as the DVD's titles: the timelines first (one with parental chapters
    /// plays its first version), the hidden title of the parental alternative not counted, the slideshow and the
    /// chapter playlist after the timelines, the parental versions as their timeline's title; the menus after them.
    /// M6.555: a project playlist has no playlist of its own (it plays its items' titles').
    #[test]
    fn a_built_blurays_playlists_read_as_the_dvds_titles() -> Result<(), Box<dyn Error>> {
        use super::bd_layout;
        use disccraft_project::{DiscKind, ParentalChapter, PlaylistItem, Project, Tick};
        use std::path::Path;
        let mut project = Project::default();
        project.settings.kind = DiscKind::BluRay;
        let movie = project.add_asset(Path::new("movie.m2v"))?;
        let other = project.add_asset(Path::new("other.m2v"))?;
        let plain = project.add_timeline(movie, Some("Plain".to_owned()))?;
        let parental = project.add_timeline(movie, Some("Parental".to_owned()))?;
        for at in [1, 2] {
            project.add_chapter(parental, Tick(at * 254_016_000_000), None)?;
        }
        project
            .timeline_mut(parental)
            .ok_or("no timeline")?
            .parental_chapters = vec![
            ParentalChapter {
                chapter: 1,
                level: 6,
                alternative: other,
            },
            ParentalChapter {
                chapter: 2,
                level: 4,
                alternative: other,
            },
        ];
        let show = project.add_slideshow(None)?;
        let list = project.add_playlist(None)?;
        project.add_playlist_item(list, plain)?;
        let chapters = project.add_chapter_playlist(parental, None)?;
        for chapter in [1, 2] {
            project.add_chapter_playlist_entry(chapters, chapter, None)?;
        }
        for item in [
            PlaylistItem::ChapterPlaylist(chapters),
            PlaylistItem::Slideshow(show),
            PlaylistItem::Timeline(parental),
        ] {
            project.add_playlist_entry(list, item, None)?;
        }
        project.add_menu(None)?;
        let layout = bd_layout(&project);
        // Playlists: 1 and 2 the timelines, 3 the hidden title, 4 the slideshow, 5 the chapter playlist, 6 and 7 the
        // parental versions (for levels 4-5 and 1-3), 8 the menu.
        let titles: Vec<(u32, (u32, Option<usize>))> = layout.titles.into_iter().collect();
        assert_eq!(
            titles,
            [
                (1, (1, None)),
                (2, (2, Some(0))),
                (4, (3, None)),
                (5, (4, None)),
                (6, (2, Some(1))),
                (7, (2, Some(2))),
            ]
        );
        assert_eq!((layout.timelines, layout.menu_base), (2, 8));
        Ok(())
    }

    /// M6.552: a built Blu-ray whose title has a parental chapter plays the title's parental version (a playlist of
    /// its own) on a player set below the chapter's level; the preview reports it as the title, in the version that
    /// replaces the chapter (as the DVD's, M13.19), and its next chapter in the same version. Uses the video the
    /// Blu-ray tests make, when it is there.
    #[test]
    fn a_built_blurays_parental_version_previews_as_its_title() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{DiscKind, Link, ParentalChapter, Tick};
        let video = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/fixtures/bd_preview.m2v");
        if !video.is_file() {
            println!(
                "skipped: no target/fixtures/bd_preview.m2v (made by the Blu-ray tests with ffmpeg)"
            );
            return Ok(());
        }
        let dir = std::env::temp_dir().join(format!(
            "disccraft-preview-bd-parental-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        // The alternative: the same video as another file (another asset).
        let alternative = dir.join("alternative.m2v");
        std::fs::copy(&video, &alternative)?;
        let mut session = Session::new();
        let p = &mut session.project;
        p.settings.kind = DiscKind::BluRay;
        let asset = p.add_asset(&video)?;
        let other = p.add_asset(&alternative)?;
        let timeline = p.add_timeline(asset, None)?;
        for at in [2, 4] {
            p.add_chapter(timeline, Tick(at * 254_016_000_000), None)?;
        }
        // Chapter 2 for level 6 (17 years); below it, the alternative.
        p.timeline_mut(timeline)
            .ok_or("no timeline")?
            .parental_chapters = vec![ParentalChapter {
            chapter: 1,
            level: 6,
            alternative: other,
        }];
        p.settings.first_play = Link::Timeline {
            id: timeline,
            chapter: 0,
        };
        let out = dir.join("out");
        let _ = disccraft_build::build(
            &session.project,
            &disccraft_build::BuildOptions {
                output: disccraft_build::OutputKind::Folder,
                path: out.clone(),
                cache: None,
                audio_language: None,
            },
        )?;
        let path = out.to_string_lossy();
        for (level, variant) in [(None, 0), (Some(8), 0), (Some(6), 0), (Some(4), 1)] {
            let params = level.map_or_else(
                || json!({ "path": path }),
                |level| json!({ "path": path, "parentalLevel": level }),
            );
            let state = session.execute("preview.open", &params)?;
            assert_eq!(
                (
                    state.pointer("/location/title/title"),
                    state.pointer("/location/title/chapter"),
                    state.get("variant"),
                ),
                (Some(&json!(1)), Some(&json!(1)), Some(&json!(variant))),
                "level {level:?}"
            );
        }
        let state = session.execute("preview.key", &json!({ "key": "next" }))?;
        assert_eq!(
            (
                state.pointer("/location/title/title"),
                state.pointer("/location/title/chapter"),
                state.get("variant"),
            ),
            (Some(&json!(1)), Some(&json!(2)), Some(&json!(1)))
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// M6.553: a built Blu-ray's project playlist plays its items as the titles they are (M6.555: their own
    /// playlists in turn, as a DVD's playlist does): a timeline at its chapters, the remote's chapter keys through
    /// them, Next on the last chapter on into the next item, the end of an item the next item, the last ending the
    /// playlist. Uses the video the Blu-ray tests make, when it is there.
    #[test]
    fn a_built_blurays_playlist_previews_its_items_as_their_titles() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{DiscKind, Link, PlaylistItem, Tick};
        let video = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/fixtures/bd_preview.m2v");
        if !video.is_file() {
            println!(
                "skipped: no target/fixtures/bd_preview.m2v (made by the Blu-ray tests with ffmpeg)"
            );
            return Ok(());
        }
        let dir = std::env::temp_dir().join(format!(
            "disccraft-preview-bd-playlist-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        let mut session = Session::new();
        let p = &mut session.project;
        p.settings.kind = DiscKind::BluRay;
        let asset = p.add_asset(&video)?;
        let first = p.add_timeline(asset, None)?;
        for at in [2, 4] {
            p.add_chapter(first, Tick(at * 254_016_000_000), None)?;
        }
        let second = p.add_timeline(asset, None)?;
        let chapters = p.add_chapter_playlist(first, None)?;
        for chapter in [2, 0] {
            p.add_chapter_playlist_entry(chapters, chapter, None)?;
        }
        let list = p.add_playlist(None)?;
        for item in [
            PlaylistItem::Timeline(first),
            PlaylistItem::ChapterPlaylist(chapters),
            PlaylistItem::Timeline(second),
        ] {
            p.add_playlist_entry(list, item, None)?;
        }
        p.settings.first_play = Link::Playlist { id: list };
        let out = dir.join("out");
        let _ = disccraft_build::build(
            &session.project,
            &disccraft_build::BuildOptions {
                output: disccraft_build::OutputKind::Folder,
                path: out.clone(),
                cache: None,
                audio_language: None,
            },
        )?;
        let at = |state: &Value| {
            (
                state
                    .pointer("/location/title/title")
                    .and_then(Value::as_u64),
                state
                    .pointer("/location/title/chapter")
                    .and_then(Value::as_u64),
            )
        };
        // Titles: 1 and 2 the timelines, 3 the chapter playlist; the playlist plays their playlists in turn.
        let state = session.execute("preview.open", &json!({ "path": out.to_string_lossy() }))?;
        assert_eq!(at(&state), (Some(1), Some(1)));
        // The picture has played into the first timeline's third chapter; Previous goes back through its chapters.
        let state = session.execute("preview.chapter", &json!({ "chapter": 3 }))?;
        assert_eq!(at(&state), (Some(1), Some(3)));
        let state = session.execute("preview.key", &json!({ "key": "previous" }))?;
        assert_eq!(at(&state), (Some(1), Some(2)));
        // Its end: the chapter playlist, its first entry, then its second; Next on its last entry: the second
        // timeline.
        let state = session.execute("preview.key", &json!({ "key": "finish" }))?;
        assert_eq!(at(&state), (Some(3), Some(1)));
        let state = session.execute("preview.key", &json!({ "key": "next" }))?;
        assert_eq!(at(&state), (Some(3), Some(2)));
        let state = session.execute("preview.key", &json!({ "key": "next" }))?;
        assert_eq!(at(&state), (Some(2), Some(1)));
        // The last item's end ends the playlist.
        let state = session.execute("preview.key", &json!({ "key": "finish" }))?;
        assert_eq!(at(&state), (None, None));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// M6.555: a built Blu-ray's project playlist asks each item's parental level and plays its version for the
    /// player's age, as the item's own title does (and a DVD's playlist): with no level set or at level 8 (21 years),
    /// the first version and then the second timeline; at level 4 (13 years), the version for it, and below the second
    /// timeline's own level 6 (17 years) what plays in its place (nothing set: the player stops). Uses the video the
    /// Blu-ray tests make, when it is there.
    #[test]
    fn a_built_blurays_playlist_asks_each_items_parental_level() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{DiscKind, Link, ParentalChapter, PlaylistItem, Tick};
        let video = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/fixtures/bd_preview.m2v");
        if !video.is_file() {
            println!(
                "skipped: no target/fixtures/bd_preview.m2v (made by the Blu-ray tests with ffmpeg)"
            );
            return Ok(());
        }
        let dir = std::env::temp_dir().join(format!(
            "disccraft-preview-bd-playlist-parental-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        let alternative = dir.join("alternative.m2v");
        std::fs::copy(&video, &alternative)?;
        let mut session = Session::new();
        let p = &mut session.project;
        p.settings.kind = DiscKind::BluRay;
        let asset = p.add_asset(&video)?;
        let other = p.add_asset(&alternative)?;
        let first = p.add_timeline(asset, None)?;
        for at in [2, 4] {
            p.add_chapter(first, Tick(at * 254_016_000_000), None)?;
        }
        p.timeline_mut(first)
            .ok_or("no timeline")?
            .parental_chapters = vec![ParentalChapter {
            chapter: 1,
            level: 6,
            alternative: other,
        }];
        let second = p.add_timeline(asset, None)?;
        p.timeline_mut(second).ok_or("no timeline")?.parental_level = Some(6);
        let list = p.add_playlist(None)?;
        for item in [
            PlaylistItem::Timeline(first),
            PlaylistItem::Timeline(second),
        ] {
            p.add_playlist_entry(list, item, None)?;
        }
        p.settings.first_play = Link::Playlist { id: list };
        let out = dir.join("out");
        let _ = disccraft_build::build(
            &session.project,
            &disccraft_build::BuildOptions {
                output: disccraft_build::OutputKind::Folder,
                path: out.clone(),
                cache: None,
                audio_language: None,
            },
        )?;
        let path = out.to_string_lossy();
        let at = |state: &Value| {
            (
                state
                    .pointer("/location/title/title")
                    .and_then(Value::as_u64),
                state
                    .pointer("/location/title/chapter")
                    .and_then(Value::as_u64),
            )
        };
        for (level, variant, second_plays) in
            [(None, 0, true), (Some(8), 0, true), (Some(4), 1, false)]
        {
            let params = level.map_or_else(
                || json!({ "path": path }),
                |level| json!({ "path": path, "parentalLevel": level }),
            );
            let state = session.execute("preview.open", &params)?;
            assert_eq!(
                (at(&state), state.get("variant")),
                ((Some(1), Some(1)), Some(&json!(variant))),
                "level {level:?}"
            );
            let state = session.execute("preview.key", &json!({ "key": "finish" }))?;
            let expected = if second_plays {
                (Some(2), Some(1))
            } else {
                (None, None)
            };
            assert_eq!(at(&state), expected, "level {level:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// A built Blu-ray opens in the preview through its own HDMV navigation, and reports where it is the way the
    /// screen draws it: the first menu (`pgcn` 1) with its buttons, the title (playlist 1) and its chapters, the
    /// title's pop-up menu (the second menu) over it. Uses the video the Blu-ray tests make, when it is there.
    #[test]
    fn a_built_bluray_opens_in_the_preview() -> Result<(), Box<dyn Error>> {
        use disccraft_project::{DiscKind, Link, Tick};
        let video = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/fixtures/bd_preview.m2v");
        if !video.is_file() {
            println!(
                "skipped: no target/fixtures/bd_preview.m2v (made by the Blu-ray tests with ffmpeg)"
            );
            return Ok(());
        }
        let mut session = Session::new();
        let p = &mut session.project;
        p.settings.kind = DiscKind::BluRay;
        let asset = p.add_asset(&video)?;
        let timeline = p.add_timeline(asset, None)?;
        p.add_chapter(timeline, Tick(3 * 254_016_000_000), None)?;
        let top = p.add_menu(Some("Top".to_owned()))?;
        if let Some(menu) = p.menus.get_mut(&top) {
            menu.buttons.clear();
        }
        for chapter in [0, 1] {
            p.add_button(
                top,
                format!("Scene {chapter}"),
                None,
                Link::Timeline {
                    id: timeline,
                    chapter,
                },
            )?;
        }
        let popup = p.add_menu(Some("Popup".to_owned()))?;
        if let Some(menu) = p.menus.get_mut(&popup) {
            menu.buttons.clear();
            menu.popup = true;
        }
        p.add_button(
            popup,
            "Menu".to_owned(),
            None,
            Link::Menu {
                id: top,
                button: None,
            },
        )?;
        if let Some(found) = p.timeline_mut(timeline) {
            found.menu_remote = Link::Menu {
                id: popup,
                button: None,
            };
        }
        p.settings.first_play = Link::Menu {
            id: top,
            button: None,
        };
        p.settings.title_remote = Link::Menu {
            id: top,
            button: None,
        };
        let out = std::env::temp_dir().join(format!("disccraft-preview-bd-{}", std::process::id()));
        let _ = disccraft_build::build(
            &session.project,
            &disccraft_build::BuildOptions {
                output: disccraft_build::OutputKind::Folder,
                path: out.clone(),
                cache: None,
                audio_language: None,
            },
        )?;

        let state = session.execute("preview.open", &json!({ "path": out.to_string_lossy() }))?;
        assert_eq!(state.get("bluray").and_then(Value::as_bool), Some(true));
        assert_eq!(
            state.pointer("/location/menu/pgcn").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            state
                .pointer("/location/menu/button")
                .and_then(Value::as_u64),
            Some(1)
        );
        let state = session.execute("preview.key", &json!({ "key": "down" }))?;
        assert_eq!(
            state
                .pointer("/location/menu/button")
                .and_then(Value::as_u64),
            Some(2)
        );
        let state = session.execute("preview.key", &json!({ "key": "enter" }))?;
        assert_eq!(
            state
                .pointer("/location/title/title")
                .and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            state
                .pointer("/location/title/chapter")
                .and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(
            state.get("popupAvailable").and_then(Value::as_bool),
            Some(true)
        );
        let state = session.execute("preview.key", &json!({ "key": "popup" }))?;
        assert_eq!(
            state.pointer("/popup/pgcn").and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(
            state.pointer("/popup/button").and_then(Value::as_u64),
            Some(1)
        );
        let state = session.execute("preview.key", &json!({ "key": "enter" }))?;
        assert_eq!(
            state.pointer("/location/menu/pgcn").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(state.get("popup"), Some(&Value::Null));
        let state = session.execute("preview.chapter", &json!({ "chapter": 1 }));
        assert!(state.is_ok());
        let state = session.execute("preview.key", &json!({ "key": "title" }))?;
        assert_eq!(
            state.pointer("/location/menu/pgcn").and_then(Value::as_u64),
            Some(1)
        );
        let _ = std::fs::remove_dir_all(&out);
        Ok(())
    }

    #[test]
    fn unknown_key_is_an_error() {
        let mut preview = Preview::default();
        assert!(preview.press("bogus").is_err());
    }

    #[test]
    fn close_resets_the_location() {
        let mut preview = Preview::default();
        preview.close();
        assert_eq!(preview.location(), Location::Stopped);
        assert!(!preview.is_open());
    }
}
