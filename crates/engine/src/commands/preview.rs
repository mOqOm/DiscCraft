//! ビルド済みディスクの VM プレビュー（`preview.*`）。
//!
//! プレビューはビルド後の `VIDEO_TS` を [`disccraft_vm`] で動かし、リモコン操作だけを
//! 受け取ります。VM はディスクを読み直すのが重いので、開いたモデルと現在の位置を
//! セッションに保持します（プロジェクト文書ではないので Undo には入りません）。

use super::always_enabled;
use crate::params::{opt_str, req_str};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_vm::{DiscModel, Key, Location, Registers, Vm, load_video_ts};
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
}

impl Default for Preview {
    fn default() -> Self {
        Self {
            model: None,
            location: Location::Stopped,
            registers: Registers::default(),
        }
    }
}

impl Preview {
    /// ディスクを開いてファーストプレイを走らせる。`start` があればそこから（Encore の「ここからプレビューする」:
    /// メニューならそのメニュー、タイトルならそのチャプターの頭。レジスタは初期値）。
    pub(crate) fn open(&mut self, path: &Path, start: Option<Location>) -> Result<(), String> {
        let model = load_video_ts(path).map_err(|error| error.to_string())?;
        let vm = match start {
            Some(location) => Vm::restore(&model, location).map_err(|error| error.to_string())?,
            None => Vm::start(&model).map_err(|error| error.to_string())?,
        };

        self.location = *vm.location();
        self.registers = vm.registers();
        self.model = Some(model);
        Ok(())
    }

    /// 読み込んだモデルを直接使う（テスト用）。
    #[cfg(test)]
    pub(crate) fn with_model(model: DiscModel) -> Result<Self, String> {
        let vm = Vm::start(&model).map_err(|error| error.to_string())?;

        Ok(Self {
            location: *vm.location(),
            registers: vm.registers(),
            model: Some(model),
        })
    }

    /// リモコンキーを送る。`finish` はタイトルを終わらせる。
    pub(crate) fn press(&mut self, key: &str) -> Result<(), String> {
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
            other => return Err(format!("unknown key \"{other}\"")),
        }
        .map_err(|error| error.to_string())?;

        self.location = *vm.location();
        self.registers = vm.registers();
        Ok(())
    }

    /// 現在の状態を JSON にする。
    pub(crate) fn state(&self) -> Value {
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
        self.model.is_some()
    }

    pub(crate) fn close(&mut self) {
        self.model = None;
        self.location = Location::Stopped;
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
        params: r#"{"path": "VIDEO_TS フォルダ", "menu"?: VMGM の PGCN, "title"?: n, "chapter"?: n}"#,
        enabled: always_enabled,
        run: run_open,
        journal: false,
    },
    CommandSpec {
        id: "preview.key",
        label: "リモコン操作",
        menu: &["プレビュー"],
        shortcut: None,
        params: r#"{"key": "up"|"down"|"left"|"right"|"enter"|"finish"|"menu"|"title"}"#,
        enabled: enabled_open,
        run: run_key,
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
    let path = req_str(params, "path")?;
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
    session.preview.open(Path::new(&path), start)?;
    Ok(session.preview.state())
}

fn run_key(session: &mut Session, params: &Value) -> CommandResult {
    let key = opt_str(params, "key")?.unwrap_or_else(|| "enter".to_string());
    session.preview.press(&key)?;
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
