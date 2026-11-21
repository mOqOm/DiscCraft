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
use disccraft_project::{TranscodeAudio, TranscodeMode, VideoStandard};
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
    /// Progress (Encore: Window ▸ Progress, F10): what is being transcoded and what media is being read.
    Progress,
    /// Character (Encore: Window ▸ Character, F3): the characters of the selected text or button.
    Character,
    /// Metadata (Encore: Window ▸ Metadata): the XMP metadata of the selected asset's file.
    Metadata,
    /// Styles (Encore: Window ▸ Styles, Ctrl+F2): text looks put on the selected text or buttons.
    Styles,
}

impl Panel {
    /// すべてのパネル。
    #[cfg(test)]
    pub(crate) const ALL: [Panel; 18] = [
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
        Panel::Progress,
        Panel::Character,
        Panel::Metadata,
        Panel::Styles,
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
            Panel::Progress => "Progress",
            Panel::Character => "Character",
            Panel::Metadata => "Metadata",
            Panel::Styles => "Styles",
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
            // As in Encore: with the properties.
            Properties | Disc | Progress | Character | Metadata => {
                &[Properties, Disc, Library, Layers]
            }
            Library | Layers | Styles => &[Library, Layers, Properties, Disc],
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
        tabs(&[Properties, Disc, Character, Metadata], 0),
        tabs(&[Library, Styles, Layers], 0),
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

/// 起動時の表示言語: 利用者が自分で選んだ言語があればそれ、なければ OS の表示言語（日本語なら日本語、英語と
/// それ以外は英語）。選んでいない間は、起動のたびに OS の言語に従う。
pub(crate) fn load_language() -> Language {
    startup_language(&load_settings(), i18n::detected)
}

/// [`load_language`] の判定。前の版は選んでいなくても言語を書き込んでいたので、`language_chosen` のない設定の
/// 言語は選んだものとは見ない。
fn startup_language(settings: &Saved, system: impl FnOnce() -> Language) -> Language {
    match settings.language {
        Some(language) if settings.language_chosen => language,
        _ => system(),
    }
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
    // 表示言語はここでは書かない（利用者が選んだときだけ `save_language` が書く）。
    save(&settings)
}

/// 表示言語を保存する（ほかの設定は維持）。
pub(crate) fn save_language(language: Language) -> Result<(), String> {
    let mut settings = load_settings();
    settings.language = Some(language);
    settings.language_chosen = true;
    save(&settings)
}

/// 環境設定 ▸ タイムライン（Encore: 静止画デュレーション / 字幕デュレーション / オーディオの言語 / 字幕の言語）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TimelinePrefs {
    /// 新しいスライドショーのスライドの長さ（秒。Encore の初期値 6）。
    #[serde(default = "default_still_seconds")]
    pub(crate) still_seconds: u32,
    /// 「字幕を追加」で足す字幕の長さ（秒。Encore の初期値 2）。
    #[serde(default = "default_subtitle_seconds")]
    pub(crate) subtitle_seconds: u32,
    /// 新しいオーディオトラックの言語（空はディスクのオーディオの言語か表示言語）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) audio_language: String,
    /// 新しい字幕トラックの言語（空はディスクのオーディオの言語か表示言語）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) subtitle_language: String,
    /// 字幕クリップをスナップ（Encore の初期値は無効）: タイムラインで動かす字幕クリップの端が、近くの字幕クリップの端、
    /// チャプターポイント、再生ヘッドに吸い付く。
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) snap_subtitles: bool,
}

fn default_still_seconds() -> u32 {
    6
}

fn default_subtitle_seconds() -> u32 {
    2
}

impl Default for TimelinePrefs {
    fn default() -> Self {
        Self {
            still_seconds: default_still_seconds(),
            subtitle_seconds: default_subtitle_seconds(),
            audio_language: String::new(),
            subtitle_language: String::new(),
            snap_subtitles: false,
        }
    }
}

impl TimelinePrefs {
    /// 入力できる範囲に収めたもの（静止画 1〜3600 秒、字幕 1〜60 秒、言語は英字 2〜3 文字か空）。
    pub(crate) fn sane(mut self) -> Self {
        self.still_seconds = self.still_seconds.clamp(1, 3600);
        self.subtitle_seconds = self.subtitle_seconds.clamp(1, 60);
        for language in [&mut self.audio_language, &mut self.subtitle_language] {
            let code: String = language
                .chars()
                .filter(char::is_ascii_alphabetic)
                .take(3)
                .collect::<String>()
                .to_ascii_lowercase();
            *language = if code.len() >= 2 { code } else { String::new() };
        }
        self
    }
}

/// Preferences ▸ Preview (Encore: TV mode, player region code, audio language): the player the previews stand
/// for. The defaults are a widescreen player of every region with no language of its own and no parental level,
/// which plays any disc as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewPrefs {
    /// A 4:3 television: a 16:9 title is letterboxed on it (Encore's TV mode "4:3 Letterbox").
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) tv_4x3: bool,
    /// On the 4:3 television, a 16:9 title is panned and scanned instead (its middle fills the screen; Encore's TV
    /// mode "4:3 Pan & Scan").
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) pan_scan: bool,
    /// The player's region (1 to 8); 0 plays every region.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) region: u8,
    /// The audio language the player prefers (a code such as `ja`); empty: none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) audio_language: String,
    /// The subtitle language the player prefers (Encore's Subtitle Language): a title with subtitles in it shows
    /// them; empty: none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) subtitle_language: String,
    /// The player's parental level (1 to 8); 0: none set.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) parental_level: u8,
    /// Encore's 再生画質をプレビュー (M6.341): how large the previews make a playing title's pictures. `None`:
    /// high, as Encore starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) quality: Option<PlaybackQuality>,
}

/// Encore's 再生画質 (Preferences ▸ General, for the Monitor) and 再生画質をプレビュー (Preferences ▸ Preview): how large
/// the pictures of a playing title are made (M6.341).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PlaybackQuality {
    /// The picture at its full size (the disc's frame).
    High,
    /// Half its size each way: quicker on a slow computer.
    Draft,
    /// Full size up to 1280 x 720; a larger (1080-line) picture at half its size, so that HD plays smoothly.
    #[default]
    Automatic,
}

impl PlaybackQuality {
    /// All of them, in Encore's order.
    pub(crate) const ALL: [PlaybackQuality; 3] = [Self::High, Self::Draft, Self::Automatic];

    /// The size the pictures of a title whose frame is `width` x `height` are played at.
    pub(crate) fn size(self, (width, height): (u32, u32)) -> (u32, u32) {
        let half = ((width / 2).max(1), (height / 2).max(1));
        match self {
            Self::High => (width, height),
            Self::Draft => half,
            Self::Automatic if width > 1280 || height > 720 => half,
            Self::Automatic => (width, height),
        }
    }

    /// The name shown in the preferences.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::High => crate::i18n::t("High"),
            Self::Draft => crate::i18n::t("Draft"),
            Self::Automatic => crate::i18n::t("Automatic"),
        }
    }
}

impl PreviewPrefs {
    /// The previews' playback quality (high unless chosen otherwise).
    pub(crate) fn quality(&self) -> PlaybackQuality {
        self.quality.unwrap_or(PlaybackQuality::High)
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &u8) -> bool {
    *value == 0
}

impl PreviewPrefs {
    /// Kept inside what can be chosen (region and level 0 to 8, the language two or three letters or empty).
    pub(crate) fn sane(mut self) -> Self {
        self.region = self.region.min(8);
        self.parental_level = self.parental_level.min(8);
        let language = |text: &str| {
            let code: String = text
                .chars()
                .filter(char::is_ascii_alphabetic)
                .take(3)
                .collect::<String>()
                .to_ascii_lowercase();
            if code.len() >= 2 { code } else { String::new() }
        };
        self.audio_language = language(&self.audio_language);
        self.subtitle_language = language(&self.subtitle_language);
        self.pan_scan &= self.tv_4x3;
        self
    }
}

/// Reads the audio output device of Preferences ▸ Audio/Video Output (`None`: the system's default).
pub(crate) fn load_audio_device() -> Option<String> {
    load_settings().audio_device.filter(|name| !name.is_empty())
}

/// Saves the audio output device (the other settings stay).
pub(crate) fn save_audio_device(name: Option<&str>) -> Result<(), String> {
    let mut settings = load_settings();
    settings.audio_device = name.filter(|name| !name.is_empty()).map(str::to_owned);
    save(&settings)
}

/// The folder the library content (`library.json`) is kept in: the one chosen in Preferences ▸ Media, else
/// `disccraft` beside the settings. `None` when the system has no settings folder and none is chosen.
pub(crate) fn library_dir() -> Option<PathBuf> {
    chosen_dir(load_settings().library_dir).or_else(default_library_dir)
}

/// The library's folder when none is chosen.
pub(crate) fn default_library_dir() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("disccraft"))
}

/// The folder what is remembered of media files is kept in: the one chosen in Preferences ▸ Media, else
/// `disccraft/media-cache` beside the settings.
pub(crate) fn media_cache_dir() -> Option<PathBuf> {
    chosen_dir(load_settings().media_cache_dir).or_else(default_media_cache_dir)
}

/// The media cache's folder when none is chosen.
pub(crate) fn default_media_cache_dir() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("disccraft").join("media-cache"))
}

/// A chosen folder that can be used: a full path (a relative one would move with the working directory).
fn chosen_dir(dir: Option<PathBuf>) -> Option<PathBuf> {
    dir.filter(|dir| dir.is_absolute())
}

/// Saves the library's folder (`None`: back beside the settings); the other settings stay.
pub(crate) fn save_library_dir(dir: Option<&Path>) -> Result<(), String> {
    let mut settings = load_settings();
    settings.library_dir = dir.map(Path::to_path_buf);
    save(&settings)
}

/// Saves the media cache's folder (`None`: back beside the settings); the other settings stay.
pub(crate) fn save_media_cache_dir(dir: Option<&Path>) -> Result<(), String> {
    let mut settings = load_settings();
    settings.media_cache_dir = dir.map(Path::to_path_buf);
    save(&settings)
}

/// Reads where the video goes out (Preferences ▸ Audio/Video Output) and whether it stays up in the background.
pub(crate) fn load_video_output() -> (Option<u32>, bool) {
    let settings = load_settings();
    (settings.video_output, settings.video_output_in_background)
}

/// Saves where the video goes out and whether it stays up in the background (the other settings stay).
pub(crate) fn save_video_output(output: Option<u32>, in_background: bool) -> Result<(), String> {
    let mut settings = load_settings();
    settings.video_output = output;
    settings.video_output_in_background = in_background;
    save(&settings)
}

/// Reads Preferences ▸ General ▸ Playback quality.
pub(crate) fn load_playback_quality() -> PlaybackQuality {
    load_settings().playback_quality.unwrap_or_default()
}

/// Saves Preferences ▸ General ▸ Playback quality (the other settings stay).
pub(crate) fn save_playback_quality(quality: PlaybackQuality) -> Result<(), String> {
    let mut settings = load_settings();
    settings.playback_quality = (quality != PlaybackQuality::default()).then_some(quality);
    save(&settings)
}

/// Reads whether the toolbar is hidden (Window ▸ Tools off).
pub(crate) fn load_hide_tools() -> bool {
    load_settings().hide_tools
}

/// Saves whether the toolbar is hidden (the other settings stay).
pub(crate) fn save_hide_tools(hide: bool) -> Result<(), String> {
    let mut settings = load_settings();
    settings.hide_tools = hide;
    save(&settings)
}

/// Reads whether HD H.264 is encoded on the system's hardware encoder (Preferences ▸ Memory, M12.40).
pub(crate) fn load_hardware_encoder() -> bool {
    load_settings().hardware_encoder
}

/// Saves whether HD H.264 is encoded on the hardware encoder (the other settings stay).
pub(crate) fn save_hardware_encoder(on: bool) -> Result<(), String> {
    let mut settings = load_settings();
    settings.hardware_encoder = on;
    save(&settings)
}

/// Reads how many transcodes run at a time (Preferences ▸ Memory); 0: one for every core.
pub(crate) fn load_parallel_transcodes() -> u32 {
    load_settings().parallel_transcodes.unwrap_or(0).min(256)
}

/// Saves how many transcodes run at a time (the other settings stay); 0: one for every core.
pub(crate) fn save_parallel_transcodes(most: u32) -> Result<(), String> {
    let mut settings = load_settings();
    settings.parallel_transcodes = (most > 0).then_some(most.min(256));
    save(&settings)
}

/// The most quality presets kept.
pub(crate) const MAX_QUALITY_PRESETS: usize = 32;

/// A quality preset (File ▸ Edit Quality Presets…): its name, its video bit rate (kbit/s), how the rate is kept
/// (Encore's presets name it: "… 7Mb VBR 2 Pass"), what the sound is made (Encore's preset's audio format), its
/// quality (Encore's quality slider, 1 to 5: how hard the encoder searches for motion) and, for a Blu-ray preset
/// (M6.328), its peak rate (kbit/s); a DVD preset has none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QualityPreset {
    pub(crate) name: String,
    pub(crate) kbps: u32,
    pub(crate) mode: TranscodeMode,
    pub(crate) audio: TranscodeAudio,
    pub(crate) quality: u8,
    /// A Blu-ray preset's peak rate (kbit/s); `None`: a DVD preset.
    pub(crate) bluray_max_kbps: Option<u32>,
}

impl QualityPreset {
    /// A DVD preset.
    pub(crate) fn dvd(
        name: &str,
        kbps: u32,
        mode: TranscodeMode,
        audio: TranscodeAudio,
        quality: u8,
    ) -> Self {
        Self {
            name: name.to_owned(),
            kbps,
            mode,
            audio,
            quality,
            bluray_max_kbps: None,
        }
    }
}

/// A preset as the settings file keeps it: its name and rate, and how the rate is kept unless it is one-pass VBR
/// (the files of older versions have none).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum StoredPreset {
    /// A Blu-ray preset: its name, rate, peak, encoding, quality and the word "bluray".
    Bluray(String, u32, u32, TranscodeMode, u8, BlurayTag),
    /// With its encoding, sound and a quality below the best.
    Rated(String, u32, TranscodeMode, TranscodeAudio, u8),
    /// With its encoding and sound.
    Full(String, u32, TranscodeMode, TranscodeAudio),
    /// With its encoding.
    Encoded(String, u32, TranscodeMode),
    /// One-pass VBR.
    Plain(String, u32),
}

/// The word that ends a stored Blu-ray preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum BlurayTag {
    Bluray,
}

impl From<StoredPreset> for QualityPreset {
    fn from(stored: StoredPreset) -> Self {
        let best = disccraft_project::BEST_QUALITY;
        let dd = TranscodeAudio::DolbyDigital;
        match stored {
            StoredPreset::Bluray(name, kbps, max_kbps, mode, quality, BlurayTag::Bluray) => Self {
                name,
                kbps,
                mode,
                audio: dd,
                quality,
                bluray_max_kbps: Some(max_kbps),
            },
            StoredPreset::Rated(name, kbps, mode, audio, quality) => {
                Self::dvd(&name, kbps, mode, audio, quality)
            }
            StoredPreset::Full(name, kbps, mode, audio) => {
                Self::dvd(&name, kbps, mode, audio, best)
            }
            StoredPreset::Encoded(name, kbps, mode) => Self::dvd(&name, kbps, mode, dd, best),
            StoredPreset::Plain(name, kbps) => Self::dvd(&name, kbps, TranscodeMode::Vbr, dd, best),
        }
    }
}

impl From<QualityPreset> for StoredPreset {
    fn from(preset: QualityPreset) -> Self {
        let QualityPreset {
            name,
            kbps,
            mode,
            audio,
            quality,
            bluray_max_kbps,
        } = preset;
        if let Some(max_kbps) = bluray_max_kbps {
            StoredPreset::Bluray(name, kbps, max_kbps, mode, quality, BlurayTag::Bluray)
        } else if quality != disccraft_project::BEST_QUALITY {
            StoredPreset::Rated(name, kbps, mode, audio, quality)
        } else if !audio.is_default() {
            StoredPreset::Full(name, kbps, mode, audio)
        } else if mode.is_default() {
            StoredPreset::Plain(name, kbps)
        } else {
            StoredPreset::Encoded(name, kbps, mode)
        }
    }
}

/// Reads the user's quality presets (name, kbit/s, encoding): at most [`MAX_QUALITY_PRESETS`], names of at most 60
/// characters, rates inside what a transcode setting takes.
pub(crate) fn load_quality_presets() -> Vec<QualityPreset> {
    sane_quality_presets(
        load_settings()
            .quality_presets
            .into_iter()
            .map(QualityPreset::from)
            .collect(),
    )
}

/// Saves the user's quality presets (the other settings stay).
pub(crate) fn save_quality_presets(presets: &[QualityPreset]) -> Result<(), String> {
    let mut settings = load_settings();
    settings.quality_presets = sane_quality_presets(presets.to_vec())
        .into_iter()
        .map(StoredPreset::from)
        .collect();
    save(&settings)
}

/// Presets as they are kept: named, within the limits.
pub(crate) fn sane_quality_presets(presets: Vec<QualityPreset>) -> Vec<QualityPreset> {
    presets
        .into_iter()
        .map(|preset| {
            let rate = |kbps: u32| {
                kbps.clamp(
                    disccraft_project::MIN_TRANSCODE_KBPS,
                    disccraft_project::MAX_TRANSCODE_KBPS,
                )
            };
            let kbps = rate(preset.kbps);
            QualityPreset {
                name: preset.name.trim().chars().take(60).collect::<String>(),
                kbps,
                mode: preset.mode,
                audio: preset.audio,
                quality: preset.quality.clamp(
                    disccraft_project::MIN_QUALITY,
                    disccraft_project::BEST_QUALITY,
                ),
                // A peak below the rate is the rate.
                bluray_max_kbps: preset.bluray_max_kbps.map(|max| rate(max).max(kbps)),
            }
        })
        .filter(|preset| !preset.name.is_empty())
        .take(MAX_QUALITY_PRESETS)
        .collect()
}

/// Reads Preferences ▸ Preview.
pub(crate) fn load_preview_prefs() -> PreviewPrefs {
    load_settings().preview_prefs.unwrap_or_default().sane()
}

/// Saves Preferences ▸ Preview (the other settings stay).
pub(crate) fn save_preview_prefs(prefs: &PreviewPrefs) -> Result<(), String> {
    let mut settings = load_settings();
    let prefs = prefs.clone().sane();
    settings.preview_prefs = (prefs != PreviewPrefs::default()).then_some(prefs);
    save(&settings)
}

/// 環境設定 ▸ タイムライン を読む（未設定は Encore の初期値）。
pub(crate) fn load_timeline_prefs() -> TimelinePrefs {
    load_settings().timeline_prefs.unwrap_or_default().sane()
}

/// 環境設定 ▸ タイムライン を保存する（ほかの設定は維持）。
pub(crate) fn save_timeline_prefs(prefs: &TimelinePrefs) -> Result<(), String> {
    let mut settings = load_settings();
    let prefs = prefs.clone().sane();
    settings.timeline_prefs = (prefs != TimelinePrefs::default()).then_some(prefs);
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
    /// 保存された表示言語。`language_chosen` のときだけ使う。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) language: Option<Language>,
    /// 利用者が表示言語を自分で選んだ（ウィンドウ ▸ 言語、環境設定）。選んでいない間は OS の表示言語に従う。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) language_chosen: bool,
    /// 最近使ったプロジェクト（新しい順）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) recent: Vec<PathBuf>,
    /// 新規プロジェクトの既定のテレビ方式（環境設定 ▸ 一般）。`None` は NTSC。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) default_standard: Option<VideoStandard>,
    /// 環境設定 ▸ タイムライン。`None` は Encore の初期値。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) timeline_prefs: Option<TimelinePrefs>,
    /// Preferences ▸ Preview. `None`: the defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) preview_prefs: Option<PreviewPrefs>,
    /// Preferences ▸ Audio/Video Output: the audio output device's name. `None`: the system's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) audio_device: Option<String>,
    /// Preferences ▸ Memory: how many transcodes run at a time. `None`: one for every core.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) parallel_transcodes: Option<u32>,
    /// File ▸ Edit Quality Presets…: the user's named video bit rates (name, kbit/s, and the encoding when it is
    /// not one-pass VBR).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) quality_presets: Vec<StoredPreset>,
    /// Preferences ▸ Audio/Video Output: where the video goes out: 0 a window of its own, `n` full screen on
    /// display `n`. `None`: nowhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) video_output: Option<u32>,
    /// Preferences ▸ Audio/Video Output: the video output stays up while DiscCraft is in the background (Encore's
    /// "Disable video output when in the background" is on by default: `false` here).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) video_output_in_background: bool,
    /// Preferences ▸ Media: the folder of the library content. `None`: beside the settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) library_dir: Option<PathBuf>,
    /// Preferences ▸ Media: the folder of the media cache. `None`: beside the settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) media_cache_dir: Option<PathBuf>,
    /// Window ▸ Tools is off: the toolbar under the menu bar is hidden (M6.339).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) hide_tools: bool,
    /// Preferences ▸ Memory: HD H.264 on the system's hardware encoder (M12.40).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) hardware_encoder: bool,
    /// Preferences ▸ General ▸ Playback quality (M6.341). `None`: automatic, as Encore starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) playback_quality: Option<PlaybackQuality>,
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

    /// Until the user chooses a language the system's is used at every start; a chosen one stays. A language an
    /// older version wrote without being asked does not count as chosen.
    /// The timeline preferences start at Encore's values and are kept within what can be typed.
    /// M6.293: the preview's player settings are kept sane: both languages two or three letters, and pan and
    /// scan only on a 4:3 television.
    #[test]
    fn preview_prefs_are_kept_sane() {
        let prefs = super::PreviewPrefs {
            tv_4x3: false,
            pan_scan: true,
            audio_language: "x".to_owned(),
            subtitle_language: " JA ".to_owned(),
            ..super::PreviewPrefs::default()
        }
        .sane();
        assert!(!prefs.pan_scan);
        assert_eq!(prefs.audio_language, "");
        assert_eq!(prefs.subtitle_language, "ja");
        let prefs = super::PreviewPrefs {
            tv_4x3: true,
            pan_scan: true,
            ..super::PreviewPrefs::default()
        }
        .sane();
        assert!(prefs.pan_scan);
    }

    #[test]
    fn timeline_preferences_have_encores_defaults_and_stay_in_range() {
        let prefs = super::TimelinePrefs::default();
        assert_eq!((prefs.still_seconds, prefs.subtitle_seconds), (6, 2));
        assert!(prefs.audio_language.is_empty() && prefs.subtitle_language.is_empty());
        // Encore: Snap subtitle clips is off.
        assert!(!prefs.snap_subtitles);
        let odd = super::TimelinePrefs {
            still_seconds: 0,
            subtitle_seconds: 500,
            audio_language: "EN-us!".to_owned(),
            subtitle_language: "j".to_owned(),
            snap_subtitles: true,
        }
        .sane();
        assert_eq!((odd.still_seconds, odd.subtitle_seconds), (1, 60));
        assert_eq!(odd.audio_language, "enu");
        assert_eq!(odd.subtitle_language, "");
        // A settings file of an older version has none.
        let old: Saved = serde_json::from_str("{}").unwrap_or_default();
        assert_eq!(old.timeline_prefs, None);
    }

    #[test]
    fn the_startup_language_follows_the_system_until_one_is_chosen() {
        let system = || Language::En;
        let mut settings = Saved::default();
        assert_eq!(super::startup_language(&settings, system), Language::En);
        settings.language = Some(Language::Ja);
        assert_eq!(super::startup_language(&settings, system), Language::En);
        settings.language_chosen = true;
        assert_eq!(super::startup_language(&settings, system), Language::Ja);
        assert_eq!(
            super::startup_language(&settings, || Language::Ja),
            Language::Ja
        );
        // An old settings file: a language and no flag.
        let old: Saved = serde_json::from_str(r#"{"language":"ja"}"#).unwrap_or_default();
        assert_eq!(old.language, Some(Language::Ja));
        assert!(!old.language_chosen);
        assert_eq!(super::startup_language(&old, system), Language::En);
    }

    #[test]
    fn the_default_workspace_shows_every_group_as_encore_does() {
        let layout = standard_layout();
        assert!(layout.validate().is_ok());
        let dock = WorkspacePreset::Standard.dock();
        for panel in Panel::ALL {
            // The chapter playlist viewer opens with a playlist (Encore opens one per playlist), and the
            // Progress panel when it is asked for (F10), as in Encore.
            assert_eq!(
                dock.layout.contains(&panel),
                !matches!(panel, Panel::ChapterPlaylist | Panel::Progress),
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

    /// M6.341: the playback qualities' sizes, and the defaults (automatic for the Monitor, high for the previews)
    /// are not written.
    #[test]
    fn playback_quality_sizes_the_pictures() {
        use super::{PlaybackQuality, PreviewPrefs};
        assert_eq!(PlaybackQuality::High.size((1920, 1080)), (1920, 1080));
        assert_eq!(PlaybackQuality::Draft.size((720, 480)), (360, 240));
        assert_eq!(PlaybackQuality::Automatic.size((1920, 1080)), (960, 540));
        assert_eq!(PlaybackQuality::Automatic.size((1280, 720)), (1280, 720));
        assert_eq!(PlaybackQuality::Automatic.size((853, 480)), (853, 480));
        assert_eq!(PlaybackQuality::Draft.size((1, 1)), (1, 1));
        assert_eq!(PreviewPrefs::default().quality(), PlaybackQuality::High);
        let shown = serde_json::to_string(&Saved::default()).unwrap_or_default();
        assert!(!shown.contains("playbackQuality"), "{shown}");
        let draft = serde_json::to_string(&Saved {
            playback_quality: Some(PlaybackQuality::Draft),
            ..Saved::default()
        })
        .unwrap_or_default();
        assert!(draft.contains("\"playbackQuality\":\"draft\""), "{draft}");
    }

    /// M6.339: Window ▸ Tools off is kept; on (the default) is not written.
    #[test]
    fn a_hidden_toolbar_is_kept() {
        let hidden = serde_json::to_string(&Saved {
            hide_tools: true,
            ..Saved::default()
        })
        .unwrap_or_default();
        assert!(hidden.contains("\"hideTools\":true"), "{hidden}");
        let saved: Saved = serde_json::from_str(&hidden).unwrap_or_default();
        assert!(saved.hide_tools);
        let shown = serde_json::to_string(&Saved::default()).unwrap_or_default();
        assert!(!shown.contains("hideTools"), "{shown}");
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
