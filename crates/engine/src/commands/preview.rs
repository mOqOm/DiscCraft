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
    /// Blu-ray のタイトルごとの、ポップアップメニュー（リモートメニューがポップアップのメニューならその PGCN）。
    /// モデルのタイトルの順（タイムライン、スライドショー、チャプター再生リスト）。
    popups: Vec<Option<u16>>,
    /// 再生中のタイトルの上に出ているポップアップメニュー（PGCN とハイライトしているボタン）。
    popup: Option<(u16, u8)>,
    /// ビルドした Blu-ray を開いているときのディスク（このときは上の DVD のモデルを使わない）。
    bd: Option<BdPreview>,
}

/// ビルドした Blu-ray のプレビュー: ナビゲーション、今の状態、プレイリストとプロジェクトの対応。
pub(crate) struct BdPreview {
    disc: BdDisc,
    state: BdState,
    /// プレイリスト 1 からこの数まではタイムラインのタイトル（画面はそのタイムラインの映像）。
    timelines: u32,
    /// メニューのプレイリストの最初の番号（プロジェクトのメニューの順に続く）。
    menu_base: u32,
    /// タイムラインごとのポップアップメニュー（メニューの順の 1 から）。
    popups: Vec<Option<u16>>,
}

/// ビルドした Blu-ray のプレイリストの番号の並び（Blu-ray のビルドと同じ）: タイムラインとスライドショーの
/// タイトル、中身のある再生リスト、中身のあるチャプター再生リスト、そしてメニューが 1 つずつ。タイムラインの数と、
/// メニューの最初の番号。
fn bd_layout(project: &Project) -> (u32, u32) {
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let timelines = count(project.timelines.len());
    let titles = timelines.saturating_add(count(project.slideshows.len()));
    let playlists = count(
        project
            .playlists
            .values()
            .filter(|list| !list.items.is_empty())
            .count(),
    );
    let chapter_playlists = count(
        project
            .chapter_playlists
            .values()
            .filter(|list| !list.chapters.is_empty())
            .count(),
    );
    let menu_base = titles
        .saturating_add(playlists)
        .saturating_add(chapter_playlists)
        .saturating_add(1);
    (timelines, menu_base)
}

/// `path` がビルドした Blu-ray（`BDMV` か、それを入れたフォルダー）か。
fn is_bdmv(path: &Path) -> bool {
    path.join("index.bdmv").is_file() || path.join("BDMV").join("index.bdmv").is_file()
}

impl BdPreview {
    fn open(path: &Path, project: &Project) -> Result<Self, String> {
        let disc = load_bdmv(path).map_err(|error| error.to_string())?;
        let state = BdVm::start(&disc)
            .map_err(|error| error.to_string())?
            .state()
            .clone();
        let (timelines, menu_base) = bd_layout(project);
        let popups = {
            let mut bluray = project.clone();
            bluray.settings.kind = DiscKind::BluRay;
            popup_menus(&bluray)
        };
        Ok(Self {
            disc,
            state,
            timelines,
            menu_base,
            popups,
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
                let BdLocation::Playing { mark, .. } = vm.state().location else {
                    return Err("not in a title".to_string());
                };
                let wanted = if key == "next" {
                    mark.saturating_add(1)
                } else {
                    mark.saturating_sub(1)
                };
                vm.jump_mark(wanted)
            }
            other => return Err(format!("unknown key \"{other}\"")),
        };
        result.map_err(|error| error.to_string())?;
        self.state = vm.state().clone();
        Ok(())
    }

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

    /// DVD のプレビューと同じ形の状態。メニューのプレイリストはプロジェクトのメニューの順（`pgcn`）、
    /// タイトルはプレイリストの番号（タイムラインの順）とマーク（チャプター）。ボタンはページの中の順の 1 から。
    fn state(&self) -> Value {
        let vm = BdVm::restore(&self.disc, self.state.clone());
        let shown = vm.menu();
        let button_number = |id: u16| -> u64 {
            shown
                .and_then(|(menu, view)| {
                    menu.pages
                        .get(usize::from(view.page))?
                        .buttons
                        .iter()
                        .position(|button| button.id == id)
                })
                .map_or(0, |index| {
                    u64::try_from(index).unwrap_or(0).saturating_add(1)
                })
        };
        let location = match self.state.location {
            BdLocation::Stopped => Value::String("stopped".to_string()),
            BdLocation::Playing { playlist, .. } if playlist >= self.menu_base => json!({
                "menu": {
                    "pgcn": playlist - self.menu_base + 1,
                    "button": shown.map_or(0, |(_, view)| button_number(view.button)),
                }
            }),
            BdLocation::Playing { playlist, mark } => json!({
                "title": { "title": playlist, "chapter": mark.saturating_add(1) }
            }),
        };
        let popup_menu = match self.state.location {
            BdLocation::Playing { playlist, .. } if playlist <= self.timelines => {
                usize::try_from(playlist)
                    .ok()
                    .and_then(|title| title.checked_sub(1))
                    .and_then(|index| self.popups.get(index))
                    .copied()
                    .flatten()
            }
            _ => None,
        };
        let popup = match (shown, popup_menu) {
            (Some((menu, view)), Some(pgcn)) if menu.popup && view.open => {
                json!({ "pgcn": pgcn, "button": button_number(view.button) })
            }
            _ => Value::Null,
        };
        let (subtitle_on, subtitle_track) = self.state.subtitle;
        json!({
            "open": true,
            "bluray": true,
            "built": true,
            "location": location,
            "popup": popup,
            "popupAvailable": vm.has_popup() && popup_menu.is_some(),
            "audio": (self.state.audio > 0).then_some(self.state.audio),
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
/// リモートメニューが「Blu-ray のポップアップメニューに設定」したボタンのあるメニューなら、その PGCN（メニューの
/// 順）。スライドショーとチャプター再生リストにはない。DVD では空。
fn popup_menus(project: &Project) -> Vec<Option<u16>> {
    if project.settings.kind != DiscKind::BluRay {
        return Vec::new();
    }
    project
        .timelines
        .values()
        .map(|timeline| match &timeline.menu_remote {
            Link::Menu { id, .. } => project
                .menus
                .get(id)
                .filter(|menu| menu.popup && !menu.buttons.is_empty())
                .and_then(|_| project.menus.keys().position(|key| key == id))
                .and_then(|index| u16::try_from(index.saturating_add(1)).ok()),
            _ => None,
        })
        .collect()
}

impl Preview {
    /// ディスクを開いてファーストプレイを走らせる。`start` があればそこから（Encore の「ここからプレビューする」:
    /// メニューならそのメニュー、タイトルならそのチャプターの頭。レジスタは初期値）。
    pub(crate) fn open(&mut self, path: &Path, start: Option<Location>) -> Result<(), String> {
        let model = load_video_ts(path).map_err(|error| error.to_string())?;
        self.open_model(model, start)
    }

    /// `model` を開いてファーストプレイ（または `start`）から走らせる。
    pub(crate) fn open_model(
        &mut self,
        model: DiscModel,
        start: Option<Location>,
    ) -> Result<(), String> {
        let vm = match start {
            Some(location) => Vm::restore(&model, location).map_err(|error| error.to_string())?,
            None => Vm::start(&model).map_err(|error| error.to_string())?,
        };

        self.location = *vm.location();
        self.registers = vm.registers();
        self.model = Some(model);
        self.bluray = false;
        self.popups = Vec::new();
        self.popup = None;
        self.bd = None;
        Ok(())
    }

    /// ビルドした Blu-ray（`BDMV` か、それを入れたフォルダー）を開いてファーストプレイから。プレイリストと
    /// `project` のタイムライン・メニューの対応はビルドと同じ並びから。
    pub(crate) fn open_bd(&mut self, path: &Path, project: &Project) -> Result<(), String> {
        let bd = BdPreview::open(path, project)?;
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

    /// 再生中のタイトルのポップアップメニューの PGCN（Blu-ray で、そのタイトルにあるとき）。
    fn title_popup(&self) -> Option<u16> {
        let Location::Title { title, .. } = self.location else {
            return None;
        };
        self.popups
            .get(usize::from(title).checked_sub(1)?)
            .copied()
            .flatten()
    }

    /// ポップアップメニューが出ている間のキー: 十字キーはそのボタンを移り、決定はボタンのリンクへ（ポップアップは
    /// 閉じる。リンクがそのポップアップ自身なら出たまま）。メニュー / ポップアップのキーで閉じる。タイトルは
    /// その間も再生を続ける（Blu-ray のポップアップメニュー）。ほかのキーはポップアップを閉じてから。
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
        let model = self.model.as_ref().ok_or("preview is not open")?;
        let mut vm =
            Vm::restore_with_registers(model, Location::Menu { pgcn, button }, self.registers)
                .map_err(|error| error.to_string())?;
        vm.press(pressed).map_err(|error| error.to_string())?;
        match *vm.location() {
            // ハイライトが動いた（または決定しても同じポップアップに残った）。
            Location::Menu {
                pgcn: shown,
                button,
            } if shown == pgcn => self.popup = Some((pgcn, button)),
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
            "next" | "previous" => {
                let Location::Title { chapter, .. } = *vm.location() else {
                    return Err("not in a title".to_string());
                };
                let wanted = if key == "next" {
                    chapter.saturating_add(1)
                } else {
                    chapter.saturating_sub(1)
                };
                vm.jump_chapter(wanted)
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
        map.insert(
            "subtitle".to_string(),
            json!({ "on": subtitle_on, "track": subtitle_track }),
        );
        map.insert("menus".to_string(), Value::Array(menus));
        map.insert("titles".to_string(), Value::Array(titles));
        Value::Object(map)
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
        label: "プレビューを開く",
        menu: &["プレビュー"],
        shortcut: None,
        params: r#"{"path"?: "VIDEO_TS フォルダ（なければビルドせずにプロジェクトから）", "menu"?: VMGM の PGCN, "title"?: n, "chapter"?: n}"#,
        enabled: always_enabled,
        run: run_open,
        journal: false,
    },
    CommandSpec {
        id: "preview.key",
        label: "リモコン操作",
        menu: &["プレビュー"],
        shortcut: None,
        params: r#"{"key": "up"|"down"|"left"|"right"|"enter"|"finish"|"menu"|"title"|"next"|"previous"}"#,
        enabled: enabled_open,
        run: run_key,
        journal: false,
    },
    CommandSpec {
        id: "preview.chapter",
        label: "チャプターへ移動",
        menu: &["プレビュー"],
        shortcut: None,
        params: r#"{"chapter": n}"#,
        enabled: enabled_open,
        run: run_chapter,
        journal: false,
    },
    CommandSpec {
        id: "preview.streams",
        label: "オーディオと字幕を選ぶ",
        menu: &["プレビュー"],
        shortcut: None,
        params: r#"{"audio"?: 1..8, "subtitle"?: 1..32 | "off"}"#,
        enabled: enabled_open,
        run: run_streams,
        journal: false,
    },
    CommandSpec {
        id: "preview.state",
        label: "プレビューの状態",
        menu: &["プレビュー"],
        shortcut: None,
        params: "{}",
        enabled: always_enabled,
        run: run_state,
        journal: false,
    },
    CommandSpec {
        id: "preview.close",
        label: "プレビューを閉じる",
        menu: &["プレビュー"],
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
    let start = if let Some(pgcn) = small("menu")? {
        Some(Location::Menu { pgcn, button: 1 })
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
            session
                .preview
                .open_bd(Path::new(&path), &session.project)?;
        }
        Some(path) => session.preview.open(Path::new(&path), start)?,
        // Encore のプロジェクトプレビュー: ビルドせずに、ビルドが書くのと同じナビゲーションで（ボタンの
        // トランジションも隠しタイトルに広げて）。
        None => {
            let project = session
                .project
                .with_transitions_expanded()
                .map_err(|error| error.to_string())?;
            let model =
                disccraft_dvd::preview_model(&project).map_err(|error| error.to_string())?;
            session.preview.open_model(model, start)?;
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
        // The chapter keys and number stay within the title's one chapter; the stream keys set SPRM1 / SPRM2.
        session.execute("preview.key", &json!({ "key": "next" }))?;
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
