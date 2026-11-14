//! パネルとワークスペース（[docs/ui-parity.md](../../../docs/ui-parity.md) §2）。
//!
//! 画面のパネルは craft-libs の craft-ui のドッキング（FilmCraft などの Craft と同じもの）で並ぶ。タブはドラッグで
//! 並べ替え・ほかのグループへ移動・分割・フローティングでき、× やタブの右クリックで閉じ、ウィンドウメニューから
//! 開き直せる（Encore のパネルと同じ）。並びはワークスペースごとに覚え、名前を付けて保存できる。
//!
//! 初期設定のワークスペース（Encore の既定の配置）:
//!
//! - 左: プロジェクト / メニュー / タイムライン / ビルド
//! - 中央: モニター / メニュービューア / フローチャート
//! - 右上: プロパティ / ディスク
//! - 右下: ライブラリ / レイヤー
//! - 下: タイムラインビューア / スライドショービューア
//!
//! 選んだワークスペースとその並び、表示言語、最近使ったプロジェクトは設定ファイルに保存する。

use crate::i18n::{self, Language, t};

use craft_ui::docking::{Action, Node, Permissions};
use craft_ui::layout::{SplitAxis, SplitSize};
use disccraft_project::VideoStandard;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 最近使ったプロジェクトを覚えておく数。
pub(crate) const RECENT_LIMIT: usize = 10;
/// 名前を付けて保存できるワークスペースの数。
pub(crate) const USER_WORKSPACE_LIMIT: usize = 32;
/// ワークスペースの名前の長さ（文字数）。
pub(crate) const WORKSPACE_NAME_LIMIT: usize = 64;

/// 画面のパネル（ドッキングのタブ）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Panel {
    /// プロジェクト（すべての項目の表）。
    Project,
    /// メニューの一覧とボタン。
    Menus,
    /// タイムラインの一覧。
    Timelines,
    /// ビルド（出力、容量、チェック）。
    Build,
    /// モニター（ビルド結果のプレビュー）。
    Monitor,
    /// メニュービューア（メニューエディター）。
    MenuViewer,
    /// フローチャート。
    Flowchart,
    /// チャプター再生リストビューア（Encore では再生リストごとのタブ）。
    ChapterPlaylist,
    /// 選択中の項目のプロパティ。
    Properties,
    /// ディスクの設定。
    Disc,
    /// ライブラリ（テンプレート）。
    Library,
    /// メニューのレイヤー（背景とボタン）。
    Layers,
    /// タイムラインビューア。
    TimelineViewer,
    /// スライドショービューア。
    SlideshowViewer,
}

impl Panel {
    /// すべてのパネル。
    #[cfg(test)]
    pub(crate) const ALL: [Panel; 14] = [
        Panel::Project,
        Panel::Menus,
        Panel::Timelines,
        Panel::Build,
        Panel::Monitor,
        Panel::MenuViewer,
        Panel::Flowchart,
        Panel::ChapterPlaylist,
        Panel::Properties,
        Panel::Disc,
        Panel::Library,
        Panel::Layers,
        Panel::TimelineViewer,
        Panel::SlideshowViewer,
    ];

    /// パネルの名前（ウィンドウメニューと同じ）。文字列は英語で書き、`i18n/ja.tsv` で日本語に引く。
    pub(crate) fn title(self) -> &'static str {
        t(match self {
            Panel::Project => "Project",
            Panel::Menus => "Menus",
            Panel::Timelines => "Timelines",
            Panel::Build => "Build",
            Panel::Monitor => "Monitor",
            Panel::MenuViewer => "Menu Viewer",
            Panel::Flowchart => "Flowchart",
            Panel::ChapterPlaylist => "Chapter Playlist",
            Panel::Properties => "Properties",
            Panel::Disc => "Disc",
            Panel::Library => "Library",
            Panel::Layers => "Layers",
            Panel::TimelineViewer => "Timeline Viewer",
            Panel::SlideshowViewer => "Slideshow Viewer",
        })
    }

    /// 閉じたパネルを開き直すとき、記録した場所がなければ並べる先（前にあるものほど優先）: 初期設定の配置で
    /// 同じグループにいるパネル、次に近いグループ。
    pub(crate) fn neighbours(self) -> &'static [Panel] {
        use Panel::*;
        match self {
            Project | Menus | Timelines | Build => &[Project, Menus, Timelines, Build, Monitor],
            Monitor | MenuViewer | Flowchart | ChapterPlaylist => {
                &[Monitor, MenuViewer, Flowchart, ChapterPlaylist, Project]
            }
            Properties | Disc => &[Properties, Disc, Library, Layers],
            Library | Layers => &[Library, Layers, Properties, Disc],
            TimelineViewer | SlideshowViewer => &[TimelineViewer, SlideshowViewer, Monitor],
        }
    }
}

/// パネルの並び（craft-ui のドッキング）。
pub(crate) type DockLayout = craft_ui::docking::Layout<Panel>;
/// ワークスペース: 今の並びと、閉じたパネルの戻り先、名前を付けたワークスペースのリセット先。
pub(crate) type Dock = craft_ui::docking::Workspace<Panel>;

/// パネルの許可: どれも動かし、フローティングにし、閉じられる。
pub(crate) fn permissions(_panel: &Panel) -> Permissions {
    Permissions::default()
}

/// 組み込みのワークスペース。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkspacePreset {
    /// 初期設定。Encore の既定の配置（5 つのグループをすべて出す）。
    #[default]
    Standard,
    /// メニュー。メニューの一覧とビューア、プロパティ、レイヤーを出す。
    Menu,
    /// ナビゲーション。フローチャートとプロパティを出す。
    Navigation,
    /// ビルド。ビルドパネルとディスクの設定を前面に出す。
    Build,
}

/// 使っているワークスペース: 組み込みのものか、名前を付けて保存したもの。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkspaceId {
    /// 組み込みのワークスペース。
    Preset(WorkspacePreset),
    /// 名前を付けて保存したワークスペース。
    User(String),
}

impl Default for WorkspaceId {
    fn default() -> Self {
        WorkspaceId::Preset(WorkspacePreset::Standard)
    }
}

impl WorkspaceId {
    /// 表示名。
    pub(crate) fn label(&self) -> String {
        match self {
            WorkspaceId::Preset(preset) => preset.label(),
            WorkspaceId::User(name) => name.clone(),
        }
    }
}

fn tabs(panels: &[Panel], active: usize) -> Node<Panel> {
    Node::Tabs {
        panels: panels.to_vec(),
        active,
    }
}

/// 左右に並べる（`ratio` は左の割合）。
fn beside(ratio: f32, left: Node<Panel>, right: Node<Panel>) -> Node<Panel> {
    Node::Split {
        axis: SplitAxis::Horizontal,
        size: SplitSize::Ratio(ratio),
        first: Box::new(left),
        second: Box::new(right),
    }
}

/// 上下に並べる（`ratio` は上の割合）。
fn above(ratio: f32, top: Node<Panel>, bottom: Node<Panel>) -> Node<Panel> {
    Node::Split {
        axis: SplitAxis::Vertical,
        size: SplitSize::Ratio(ratio),
        first: Box::new(top),
        second: Box::new(bottom),
    }
}

/// Encore の既定の配置: 右の列が横幅の 22%（上下ほぼ半分）、その左の下に高さ 36% の下のグループ、上に左（全体の
/// 35%）と中央。
pub(crate) fn standard_layout() -> DockLayout {
    use Panel::*;
    let left = tabs(&[Project, Menus, Timelines, Build], 0);
    let center = tabs(&[Monitor, MenuViewer, Flowchart], 0);
    // Encore と同じく、右下はライブラリが前。
    let right = above(
        0.51,
        tabs(&[Properties, Disc], 0),
        tabs(&[Library, Layers], 0),
    );
    let bottom = tabs(&[TimelineViewer, SlideshowViewer], 0);
    // 左は全体の 35%: 右の列を除いた 78% のうちの 0.45。
    let main = above(0.64, beside(0.45, left, center), bottom);
    DockLayout {
        root: Some(beside(0.78, main, right)),
        floating: Vec::new(),
    }
}

impl WorkspacePreset {
    /// 全プリセット（メニューに表示する順）。
    pub(crate) const ALL: [WorkspacePreset; 4] = [
        WorkspacePreset::Standard,
        WorkspacePreset::Menu,
        WorkspacePreset::Navigation,
        WorkspacePreset::Build,
    ];

    /// 表示名。文字列は英語で書き、`i18n/ja.tsv` で日本語に引く。
    pub(crate) fn label(self) -> String {
        t(match self {
            WorkspacePreset::Standard => "Default",
            WorkspacePreset::Menu => "Menu",
            WorkspacePreset::Navigation => "Navigation",
            WorkspacePreset::Build => "Build",
        })
        .to_string()
    }

    /// プリセットのワークスペース: 初期設定の配置から、前面に出すタブを選び、使わないパネルを閉じたもの
    /// （閉じたパネルは初期設定の場所へ戻る）。
    pub(crate) fn dock(self) -> Dock {
        use Panel::*;
        let (front, closed): (&[Panel], &[Panel]) = match self {
            WorkspacePreset::Standard => (&[], &[]),
            WorkspacePreset::Menu => (
                &[Menus, MenuViewer, Layers],
                &[TimelineViewer, SlideshowViewer],
            ),
            WorkspacePreset::Navigation => (
                &[Flowchart],
                &[Library, Layers, TimelineViewer, SlideshowViewer],
            ),
            WorkspacePreset::Build => (
                &[Build, Disc],
                &[Library, Layers, TimelineViewer, SlideshowViewer],
            ),
        };
        let mut dock = Dock::new(standard_layout());
        let actions = front
            .iter()
            .map(|panel| Action::Activate { panel: *panel })
            .chain(closed.iter().map(|panel| Action::Close { panel: *panel }));
        // 組み込みの配置なので失敗しない。失敗したら初期設定の配置のまま。
        if dock.apply_all(actions, permissions).is_err() {
            dock = Dock::new(standard_layout());
        }
        dock
    }

    /// プリセットの並び（リセットの戻り先）。
    pub(crate) fn layout(self) -> DockLayout {
        self.dock().layout
    }
}

/// 設定ファイルの場所。取れないときは `None`（保存と読み込みをスキップする）。
pub(crate) fn config_path() -> Option<PathBuf> {
    let mut path = config_dir()?;
    path.push("disccraft");
    path.push("workspace.json");
    Some(path)
}

pub(crate) fn config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }

    #[cfg(not(windows))]
    {
        if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(dir));
        }

        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
    }
}

/// 保存された設定を読む。失敗時は既定（プリセットは初期設定、言語はシステム言語）。
pub(crate) fn load_settings() -> Saved {
    let Some(path) = config_path() else {
        return Saved::default();
    };

    let Ok(text) = std::fs::read_to_string(path) else {
        return Saved::default();
    };

    serde_json::from_str::<Saved>(&text).unwrap_or_default()
}

/// 使っていたワークスペースとその並び、名前を付けて保存したワークスペースの名前を読む。並びが読めないとき
/// （ないとき、壊れているとき、新しい版のもの）はそのワークスペースの初期の並び。
pub(crate) fn load_workspace() -> (WorkspaceId, Dock, Vec<String>) {
    let settings = load_settings();
    let mut users = settings.user_workspaces.clone();
    users.truncate(USER_WORKSPACE_LIMIT);
    let mut id = settings
        .current
        .clone()
        .unwrap_or(WorkspaceId::Preset(settings.workspace));
    if let WorkspaceId::User(name) = &id
        && !users.contains(name)
    {
        id = WorkspaceId::Preset(settings.workspace);
    }
    let dock = saved_dock(&settings, &id).unwrap_or_else(|| default_dock(&settings, &id));
    (id, dock, users)
}

/// `id` のワークスペースの保存した並び（読めて、正しいときだけ）。
pub(crate) fn saved_dock(settings: &Saved, id: &WorkspaceId) -> Option<Dock> {
    let saved = settings.arrangements.iter().find(|saved| &saved.id == id)?;
    let dock = serde_json::from_value::<Dock>(saved.dock.clone()).ok()?;
    dock.validate().ok()?;
    Some(dock)
}

/// `id` のワークスペースの初期の並び: プリセットはその配置、名前を付けたものは保存したときの並び（なければ
/// 初期設定）。
pub(crate) fn default_dock(settings: &Saved, id: &WorkspaceId) -> Dock {
    match id {
        WorkspaceId::Preset(preset) => preset.dock(),
        WorkspaceId::User(_) => saved_dock(settings, id)
            .and_then(|dock| dock.saved.map(Dock::new))
            .unwrap_or_else(|| WorkspacePreset::Standard.dock()),
    }
}

/// リセットの戻り先: プリセットはその配置（名前を付けたものは `Dock::saved` が先に使われる）。
pub(crate) fn reset_layout(id: &WorkspaceId) -> DockLayout {
    match id {
        WorkspaceId::Preset(preset) => preset.layout(),
        WorkspaceId::User(_) => standard_layout(),
    }
}

/// 保存された表示言語を読む。保存がなければシステム言語。
pub(crate) fn load_language() -> Language {
    load_settings().language.unwrap_or_else(i18n::detected)
}

/// 最近使ったプロジェクト（新しい順）。
pub(crate) fn load_recent() -> Vec<PathBuf> {
    load_settings().recent
}

/// 設定を保存する。失敗しても編集は止めない。
fn save(settings: &Saved) -> Result<(), String> {
    let path = config_path().ok_or_else(|| t("Cannot find where to save settings").to_string())?;

    if let Some(dir) = path.parent()
        && let Err(error) = std::fs::create_dir_all(dir)
    {
        return Err(error.to_string());
    }

    let text = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;

    std::fs::write(&path, text).map_err(|error| error.to_string())
}

/// 使っているワークスペースとその並び、名前を付けたワークスペースの一覧を保存する（ほかの設定は維持）。
pub(crate) fn save_workspace(
    id: &WorkspaceId,
    dock: &Dock,
    users: &[String],
) -> Result<(), String> {
    let mut settings = load_settings();
    if let WorkspaceId::Preset(preset) = id {
        settings.workspace = *preset;
    }
    settings.current = Some(id.clone());
    settings.user_workspaces = users.to_vec();
    let value = serde_json::to_value(dock).map_err(|error| error.to_string())?;
    settings.arrangements.retain(|saved| &saved.id != id);
    settings.arrangements.push(SavedDock {
        id: id.clone(),
        dock: value,
    });
    // 消したワークスペースの並びは残さない。
    settings.arrangements.retain(|saved| match &saved.id {
        WorkspaceId::Preset(_) => true,
        WorkspaceId::User(name) => users.contains(name),
    });
    settings.language = Some(i18n::current());
    save(&settings)
}

/// 表示言語を保存する（ほかの設定は維持）。
pub(crate) fn save_language(language: Language) -> Result<(), String> {
    let mut settings = load_settings();
    settings.language = Some(language);
    save(&settings)
}

/// 新規プロジェクトの既定のテレビ方式を読む（未設定は NTSC）。
pub(crate) fn load_default_standard() -> VideoStandard {
    load_settings().default_standard.unwrap_or_default()
}

/// 新規プロジェクトの既定のテレビ方式を保存する（ほかの設定は維持）。
pub(crate) fn save_default_standard(standard: VideoStandard) -> Result<(), String> {
    let mut settings = load_settings();
    settings.default_standard = Some(standard);
    save(&settings)
}

/// 最近使ったプロジェクトの先頭に `path` を足して保存し、新しい一覧を返す。
pub(crate) fn remember_recent(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut settings = load_settings();
    crate::welcome::add_recent(&mut settings.recent, path, RECENT_LIMIT);
    save(&settings)?;
    Ok(settings.recent)
}

/// 表の列の幅を保存する（ほかの設定は維持）。
pub(crate) fn save_column_widths(widths: &crate::theme::ColumnWidths) -> Result<(), String> {
    let mut settings = load_settings();
    settings.column_widths.clone_from(widths);
    save(&settings)
}

/// 最近使ったプロジェクトから `path` を外して保存し、新しい一覧を返す。
pub(crate) fn forget_recent(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut settings = load_settings();
    settings.recent.retain(|existing| existing != path);
    save(&settings)?;
    Ok(settings.recent)
}

/// ワークスペースの並び。読めないもの（新しい版のパネルなど）があってもほかを失わないよう JSON のまま持つ。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SavedDock {
    /// どのワークスペースの並びか。
    pub(crate) id: WorkspaceId,
    /// craft-ui の `Workspace`。
    pub(crate) dock: serde_json::Value,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Saved {
    /// 最後に使ったプリセット（前の版の設定ファイルはこれだけを持つ）。
    #[serde(default)]
    pub(crate) workspace: WorkspacePreset,
    /// 使っているワークスペース。`None` は `workspace` のプリセット。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) current: Option<WorkspaceId>,
    /// 名前を付けて保存したワークスペース（作った順）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) user_workspaces: Vec<String>,
    /// ワークスペースごとの今の並び。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) arrangements: Vec<SavedDock>,
    /// 表の列の幅（「表:列」ごと。見出しの右端をドラッグして決めたもの）。
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub(crate) column_widths: crate::theme::ColumnWidths,
    /// 保存された表示言語。`None` は未設定（システム言語を使う）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) language: Option<Language>,
    /// 最近使ったプロジェクト（新しい順）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) recent: Vec<PathBuf>,
    /// 新規プロジェクトの既定のテレビ方式（環境設定 ▸ 一般）。`None` は NTSC。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) default_standard: Option<VideoStandard>,
}

#[cfg(test)]
mod tests {
    use super::{
        Dock, Panel, Saved, SavedDock, WorkspaceId, WorkspacePreset, default_dock, permissions,
        saved_dock, standard_layout,
    };
    use crate::i18n::{self, Language};
    use craft_ui::docking::{Action, PanelState};
    use std::path::PathBuf;

    fn front(dock: &Dock, panel: Panel) -> bool {
        dock.layout.panel_state(&panel) == PanelState::Front
    }

    #[test]
    fn the_default_workspace_shows_every_group_as_encore_does() {
        let layout = standard_layout();
        assert!(layout.validate().is_ok());
        let dock = WorkspacePreset::Standard.dock();
        for panel in Panel::ALL {
            // The chapter playlist viewer opens with a playlist (Encore opens one per playlist).
            assert_eq!(
                dock.layout.contains(&panel),
                panel != Panel::ChapterPlaylist,
                "{panel:?}"
            );
        }
        for panel in [
            Panel::Project,
            Panel::Monitor,
            Panel::Properties,
            Panel::Library,
            Panel::TimelineViewer,
        ] {
            assert!(front(&dock, panel), "{panel:?}");
        }
    }

    #[test]
    fn presets_bring_their_panels_forward_and_close_the_others() {
        let menu = WorkspacePreset::Menu.dock();
        assert!(front(&menu, Panel::Menus) && front(&menu, Panel::MenuViewer));
        assert!(front(&menu, Panel::Layers) && front(&menu, Panel::Properties));
        assert!(!menu.layout.contains(&Panel::TimelineViewer));

        let navigation = WorkspacePreset::Navigation.dock();
        assert!(front(&navigation, Panel::Flowchart));
        assert!(!navigation.layout.contains(&Panel::Library));

        let build = WorkspacePreset::Build.dock();
        assert!(front(&build, Panel::Build) && front(&build, Panel::Disc));
        assert!(!build.layout.contains(&Panel::SlideshowViewer));
    }

    #[test]
    fn a_closed_panel_comes_back_where_it_was() {
        // The Build preset closed the bottom group; reopening the timeline viewer puts it back under the
        // left and centre groups, as in the default workspace.
        let mut dock = WorkspacePreset::Build.dock();
        assert!(
            dock.set_visible(Panel::TimelineViewer, true, None, permissions)
                .is_ok()
        );
        assert!(front(&dock, Panel::TimelineViewer));
        // Dragged into the left group, closed, and reopened: it goes back into the left group.
        let mut dock = WorkspacePreset::Standard.dock();
        let moved = dock.apply(
            Action::Move {
                panel: Panel::Disc,
                anchor: Panel::Project,
                placement: craft_ui::docking::Placement::Tab { before: None },
            },
            permissions,
        );
        assert!(moved.is_ok(), "{moved:?}");
        assert!(
            dock.apply(Action::Close { panel: Panel::Disc }, permissions)
                .is_ok()
        );
        assert!(
            dock.set_visible(Panel::Disc, true, None, permissions)
                .is_ok()
        );
        assert!(front(&dock, Panel::Disc));
    }

    #[test]
    fn arrangements_are_saved_per_workspace_and_bad_ones_fall_back() {
        let mut dock = WorkspacePreset::Standard.dock();
        assert!(
            dock.apply(
                Action::Close {
                    panel: Panel::Library
                },
                permissions
            )
            .is_ok()
        );
        let id = WorkspaceId::User("Mine".to_owned());
        let settings = Saved {
            current: Some(id.clone()),
            user_workspaces: vec!["Mine".to_owned()],
            arrangements: vec![
                SavedDock {
                    id: id.clone(),
                    dock: serde_json::to_value(&dock).unwrap_or_default(),
                },
                SavedDock {
                    id: WorkspaceId::Preset(WorkspacePreset::Menu),
                    dock: serde_json::json!({"layout": {"root": 7}}),
                },
            ],
            ..Saved::default()
        };
        let text = serde_json::to_string(&settings).unwrap_or_default();
        let read: Saved = serde_json::from_str(&text).unwrap_or_default();
        assert_eq!(saved_dock(&read, &id), Some(dock));
        // An arrangement that does not read falls back to the preset's own.
        let menu = WorkspaceId::Preset(WorkspacePreset::Menu);
        assert_eq!(saved_dock(&read, &menu), None);
        assert_eq!(
            default_dock(&read, &menu).layout,
            WorkspacePreset::Menu.layout()
        );
    }

    #[test]
    fn saved_preset_round_trips() {
        let json = serde_json::to_string(&WorkspacePreset::Navigation)
            .unwrap_or_else(|_| String::from("null"));
        assert_eq!(json, "\"navigation\"");

        let parsed: WorkspacePreset = serde_json::from_str("\"build\"").unwrap_or_default();
        assert_eq!(parsed, WorkspacePreset::Build);
    }

    #[test]
    fn saved_settings_keep_the_language_and_recent_projects() {
        let text = serde_json::to_string(&Saved {
            workspace: WorkspacePreset::Menu,
            language: Some(Language::En),
            recent: vec![PathBuf::from("a.dcproj")],
            ..Saved::default()
        })
        .unwrap_or_else(|_| String::from("null"));

        assert!(text.contains("\"language\":\"en\""), "{text}");

        let saved: Saved = serde_json::from_str(&text).unwrap_or_default();
        assert_eq!(saved.workspace, WorkspacePreset::Menu);
        assert_eq!(saved.language, Some(Language::En));
        assert_eq!(saved.recent, vec![PathBuf::from("a.dcproj")]);
    }

    #[test]
    fn settings_without_a_language_stay_unset() {
        let saved: Saved = serde_json::from_str("{\"workspace\":\"standard\"}").unwrap_or_default();

        assert_eq!(saved.workspace, WorkspacePreset::Standard);
        assert_eq!(saved.language, None);
        assert!(saved.recent.is_empty());
        assert!(saved.current.is_none() && saved.arrangements.is_empty());

        // 言語と最近使ったプロジェクトが未設定なら JSON にも出さない。
        let text =
            serde_json::to_string(&Saved::default()).unwrap_or_else(|_| String::from("null"));

        assert!(!text.contains("language"), "{text}");
        assert!(!text.contains("recent"), "{text}");
    }

    #[test]
    fn preset_labels_are_translated() {
        i18n::set_current(Language::Ja);
        assert_eq!(WorkspacePreset::Standard.label(), "初期設定");
        assert_eq!(Panel::TimelineViewer.title(), "タイムラインビューア");

        i18n::set_current(Language::En);
        assert_eq!(WorkspacePreset::Standard.label(), "Default");

        i18n::set_current(Language::Ja);
    }
}
