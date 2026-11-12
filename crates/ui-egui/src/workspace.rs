//! ワークスペースプリセット（[docs/ui-parity.md](../../../docs/ui-parity.md) §2）。
//!
//! Encore の「ワークスペース」切り替えに当たるもの。画面は 5 つのタブグループからなる。
//!
//! - 左: プロジェクト / メニュー / タイムライン / ビルド
//! - 中央: モニター / メニュー（ビューア） / フローチャート
//! - 右上: プロパティ / ディスク
//! - 右下: レイヤー / ライブラリ
//! - 下: タイムライン（ビューア） / スライドショー（ビューア）
//!
//! プリセットはグループの表示と前面のタブを決める。選んだプリセット、表示言語、最近使ったプロジェクトは
//! 設定ファイルに保存する。

use crate::i18n::{self, Language, t};

use disccraft_project::VideoStandard;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 最近使ったプロジェクトを覚えておく数。
pub(crate) const RECENT_LIMIT: usize = 10;

/// ワークスペースの名前。
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

/// 左のグループのタブ。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum LeftTab {
    /// プロジェクト（すべての項目の表）。
    #[default]
    Project,
    /// メニューの一覧とボタン。
    Menus,
    /// タイムラインの一覧。
    Timelines,
    /// ビルド（出力、容量、チェック）。
    Build,
}

/// 中央のグループのタブ。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CenterTab {
    /// モニター（ビルド結果のプレビュー）。
    #[default]
    Monitor,
    /// メニュービューア（メニューエディター）。
    Menu,
    /// フローチャート。
    Flowchart,
    /// チャプター再生リストビューア（Encore では再生リストごとのタブ）。
    ChapterPlaylist,
}

/// 右上のグループのタブ。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum RightTab {
    /// 選択中の項目のプロパティ。
    #[default]
    Properties,
    /// ディスクの設定。
    Disc,
}

/// 右下のグループのタブ。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum LowerTab {
    /// メニューのレイヤー（背景とボタン）。
    #[default]
    Layers,
    /// ライブラリ（テンプレート）。
    Library,
}

/// 下のグループのタブ。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum BottomTab {
    /// タイムラインビューア。
    #[default]
    Timeline,
    /// スライドショービューア。
    Slideshow,
}

/// グループの表示と前面のタブ。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Layout {
    /// 左のグループを表示する。
    pub(crate) left: bool,
    /// 右上のグループを表示する。
    pub(crate) right: bool,
    /// 右下のグループを表示する。
    pub(crate) lower: bool,
    /// 下のグループを表示する。
    pub(crate) bottom: bool,
    /// 中央の前面のタブ。
    pub(crate) tab: CenterTab,
    /// 左の前面のタブ。
    pub(crate) left_tab: LeftTab,
    /// 右上の前面のタブ。
    pub(crate) right_tab: RightTab,
    /// 右下の前面のタブ。
    pub(crate) lower_tab: LowerTab,
    /// 下の前面のタブ。
    pub(crate) bottom_tab: BottomTab,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            left: true,
            right: true,
            lower: true,
            bottom: true,
            tab: CenterTab::Monitor,
            left_tab: LeftTab::Project,
            right_tab: RightTab::Properties,
            // Encore の初期設定と同じく、右下はライブラリが前。
            lower_tab: LowerTab::Library,
            bottom_tab: BottomTab::Timeline,
        }
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

    /// プリセットのパネル構成。
    pub(crate) fn layout(self) -> Layout {
        let base = Layout::default();
        match self {
            WorkspacePreset::Standard => base,
            WorkspacePreset::Menu => Layout {
                bottom: false,
                tab: CenterTab::Menu,
                left_tab: LeftTab::Menus,
                lower_tab: LowerTab::Layers,
                ..base
            },
            WorkspacePreset::Navigation => Layout {
                lower: false,
                bottom: false,
                tab: CenterTab::Flowchart,
                ..base
            },
            WorkspacePreset::Build => Layout {
                lower: false,
                bottom: false,
                left_tab: LeftTab::Build,
                right_tab: RightTab::Disc,
                ..base
            },
        }
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

/// 保存されたプリセットを読む。失敗時は既定。
pub(crate) fn load_preset() -> WorkspacePreset {
    load_settings().workspace
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

/// プリセットを保存する（ほかの設定は維持）。
pub(crate) fn save_preset(preset: WorkspacePreset) -> Result<(), String> {
    let mut settings = load_settings();
    settings.workspace = preset;
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

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Saved {
    #[serde(default)]
    pub(crate) workspace: WorkspacePreset,
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
    use super::{BottomTab, CenterTab, LeftTab, RightTab, Saved, WorkspacePreset};
    use crate::i18n::{self, Language};
    use std::path::PathBuf;

    #[test]
    fn presets_cover_every_tab() {
        let tabs: Vec<CenterTab> = WorkspacePreset::ALL
            .iter()
            .map(|preset| preset.layout().tab)
            .collect();

        assert!(tabs.contains(&CenterTab::Menu));
        assert!(tabs.contains(&CenterTab::Flowchart));
        assert!(tabs.contains(&CenterTab::Monitor));
    }

    #[test]
    fn default_preset_shows_every_group() {
        let layout = WorkspacePreset::Standard.layout();
        assert!(layout.left && layout.right && layout.lower && layout.bottom);
        assert_eq!(layout.left_tab, LeftTab::Project);
        assert_eq!(layout.tab, CenterTab::Monitor);
        assert_eq!(layout.right_tab, RightTab::Properties);
        assert_eq!(layout.bottom_tab, BottomTab::Timeline);
    }

    #[test]
    fn build_preset_brings_build_and_disc_forward() {
        let layout = WorkspacePreset::Build.layout();
        assert_eq!(layout.left_tab, LeftTab::Build);
        assert_eq!(layout.right_tab, RightTab::Disc);
        assert!(!layout.bottom);
    }

    #[test]
    fn menu_preset_focuses_the_canvas() {
        let layout = WorkspacePreset::Menu.layout();
        assert!(layout.left);
        assert!(layout.right);
        assert!(!layout.bottom);
        assert_eq!(layout.tab, CenterTab::Menu);
        assert_eq!(layout.left_tab, LeftTab::Menus);
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
            default_standard: None,
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

        i18n::set_current(Language::En);
        assert_eq!(WorkspacePreset::Standard.label(), "Default");

        i18n::set_current(Language::Ja);
    }
}
