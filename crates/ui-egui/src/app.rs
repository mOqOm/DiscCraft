use crate::build_panel::{
    BuildJob, ProblemView, SimulateSummary, Usage, parse_problems, parse_usage,
};
use crate::i18n::{Language, tl, tlf};
use crate::menu_editor::{CanvasDrag, HighlightPreview, MenuTexture};
use crate::menubar::ViewRequest;
use crate::monitor::PreviewState;
use crate::welcome::{Welcome, WelcomeChoice};
use crate::workspace::{Dock, Panel, WorkspaceId, load_language, load_recent, load_workspace};

use disccraft_build::{BuildReport, OutputKind};
use disccraft_engine::{Session, find_command};
use disccraft_project::{
    AssetId, Capacity, DiscKind, Link, MenuId, Rect, TimelineId, VideoStandard,
};

use egui::{self, Context, Ui};
use rfd::FileDialog;
use serde_json::{self, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Current central-panel selection.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Selection {
    None,
    Asset(AssetId),
    Timeline(TimelineId),
    Menu(MenuId),
    Slideshow(u64),
    Playlist(u64),
    /// A chapter playlist.
    ChapterPlaylist(u64),
}

/// Desktop application state.
pub struct DiscCraftApp {
    pub(crate) session: Session,
    pub(crate) selection: Selection,
    /// The item copied in the Project panel (Copy in its right-click menu), for Paste to make a copy of.
    pub(crate) project_clipboard: Selection,
    pub(crate) status: String,
    pub(crate) language: Language,
    /// 使っているワークスペース。
    pub(crate) workspace: WorkspaceId,
    /// 今のパネルの並び（craft-ui のドッキング。ワークスペースから始まり、ドラッグやウィンドウメニューで変わる）。
    pub(crate) dock: Dock,
    /// 並びが変わって、まだ設定に保存していない。
    pub(crate) dock_dirty: bool,
    /// フレームを最大化 で最大化しているフレームのパネル（Encore と同じく、もう一度選ぶと元の大きさに戻る）。
    pub(crate) maximized_panel: Option<crate::workspace::Panel>,
    /// 前のフレームで描いた各パネルの中身の場所（パネルのメニューの ドッキングを解除 で、浮かべる窓の場所と大きさ）。
    pub(crate) panel_bodies: Vec<(crate::workspace::Panel, egui::Rect)>,
    /// 名前を付けて保存したワークスペース（作った順）。
    pub(crate) user_workspaces: Vec<String>,
    /// 「新規ワークスペース…」ダイアログの名前の欄（開いているときだけ）。
    pub(crate) new_workspace: Option<String>,
    /// 開いたプロジェクトのタイムラインの波形を、次のフレームで裏で作り始める。
    pub(crate) preload_waveforms: bool,
    /// 次のフレームで名前の入力欄にフォーカスを移す（名前の変更）。
    pub(crate) focus_rename: bool,
    /// タイムラインのビューへの依頼（ズーム、チャプターポイントの追加と消去）。
    pub(crate) view_request: Option<ViewRequest>,
    /// 最後に開いたメニュー / タイムライン / スライドショー（ビューアは選択が変わっても表示を続ける）。
    pub(crate) last_menu: Option<MenuId>,
    pub(crate) last_timeline: Option<TimelineId>,
    pub(crate) last_slideshow: Option<u64>,
    pub(crate) last_chapter_playlist: Option<u64>,
    /// チャプター再生リストビューアで選んだマスターのチャプター（再生リスト, チャプター番号）とエントリ。
    pub(crate) chapter_playlist_pick: Option<(u64, usize)>,
    /// M6.425: every master chapter picked (Shift / Ctrl+click, Ctrl+A) for the chapter playlist of
    /// `chapter_playlist_pick`, which is the anchor of a Shift+click range.
    pub(crate) chapter_playlist_picks: Vec<usize>,
    pub(crate) chapter_entry_pick: Option<(u64, usize)>,
    /// アセットの `asset.probe` の結果（キーはアセット ID）。
    pub(crate) durations: BTreeMap<u64, crate::project_panel::ProbeInfo>,
    /// アセットのサムネール（`asset.thumbnail` の結果。作れなかったものは `None`）。
    pub(crate) thumbnails: BTreeMap<u64, Option<egui::TextureHandle>>,
    /// 映像アセットの最後のフレーム（タイムラインのビデオトラックの末尾に描く）。
    pub(crate) last_frames: BTreeMap<u64, Option<egui::TextureHandle>>,
    /// オーディオトラックの波形（アセットごと、別のスレッドで作る）。
    pub(crate) waveforms: crate::waveforms::Waveforms,
    /// The pictures along the video clips in the Timeline viewer (M6.483).
    pub(crate) clip_thumbs: crate::clip_thumbs::ClipThumbs,
    /// 起動時のようこそ画面と新規プロジェクトダイアログ。
    pub(crate) welcome: Welcome,
    /// 最近使ったプロジェクト（新しい順）。
    pub(crate) recent: Vec<PathBuf>,
    /// 「プロジェクトをチェック」のウィンドウ。
    pub(crate) check_window: crate::check_window::CheckWindow,
    /// 未保存の変更の確認のあとに実行する操作。
    pub(crate) pending_action: Option<crate::confirm::PendingAction>,
    /// ビルドの前のチェックで問題が見つかり、「表示 / 無視して続行 / キャンセル」を出しているか。
    pub(crate) build_prompt: bool,
    /// 失敗したビルドのエラー。OK を押すまでダイアログに出す（M6.503）。
    pub(crate) build_failure: Option<String>,
    /// タイムライン ▸ FilmCraft のマーカーを読み込み… のダイアログ（開いている間だけ）。
    pub(crate) film_marker_dialog: Option<crate::filmcraft::FilmMarkerDialog>,
    /// FilmCraft で書き出している最中のシーケンス（終わるとタイムラインになる）。
    pub(crate) film_export: Option<crate::filmcraft::FilmExport>,
    /// 確認済みで、ウィンドウを閉じてよい。
    pub(crate) allow_close: bool,
    /// 環境設定ダイアログ。
    pub(crate) prefs: crate::prefs::PrefsWindow,
    /// 環境設定 ▸ タイムライン（起動時に読み、変えたら保存する）。
    pub(crate) timeline_prefs: crate::workspace::TimelinePrefs,
    /// Preferences ▸ Preview: the player the previews stand for.
    pub(crate) preview_prefs: crate::workspace::PreviewPrefs,
    /// ライブラリパネルの状態。
    pub(crate) library: crate::library_panel::LibraryState,
    /// ユーザーがライブラリに足した項目（`library.json`）。
    pub(crate) user_library: crate::user_library::UserLibrary,
    /// The characters of the next text typed (Encore: the Character panel with nothing selected), as the fields
    /// `menu.setText` takes; empty: the defaults.
    pub(crate) next_text: serde_json::Map<String, serde_json::Value>,
    /// The clip chosen in the timeline viewer (Edit ▸ Clear and Ripple Delete act on it).
    pub(crate) clip_selection: Option<(TimelineId, crate::timeline_view::ClipPick)>,
    /// The Library's New Set / Rename Set dialog while it is open.
    pub(crate) library_set_dialog: Option<crate::library_panel::SetNameDraft>,
    /// Files being taken into the Library (M6.403: Add Items from Files… / Import Folder…), and the set the files
    /// chosen one by one go into.
    pub(crate) library_import: Option<(crate::library_files::LibraryImport, String)>,
    /// M6.491: the Blu-ray's subtitles being taken out after an import.
    pub(crate) bluray_subtitles: Option<crate::bluray_import::SubtitleTaking>,
    /// The previews of the Library's own files, by file (`None`: it could not be drawn).
    pub(crate) library_previews: std::collections::HashMap<String, Option<egui::TextureHandle>>,
    /// 「一定間隔でチャプターポイントを追加」ダイアログ。
    pub(crate) chapter_interval: Option<crate::context_menus::ChapterInterval>,
    /// モニターに映しているタイムラインのフレーム（アセット ID、フレーム番号、テクスチャ）。
    pub(crate) monitor_frame: Option<(u64, i64, egui::TextureHandle)>,
    /// モニターの静止画を裏のスレッドでデコードする（スクラブで画面を止めない。開いた映像は使い回す）。
    pub(crate) monitor_loader: disccraft_engine::FrameLoader,
    /// いまデコードを頼んでいるモニターのフレーム（アセット ID とフレーム番号、大きさ）。
    pub(crate) monitor_wanted: Option<((u64, i64), (u32, u32))>,
    /// モニターに絵を出せない理由（デコードできない映像。黒い画面の上に出す）。
    pub(crate) monitor_note: Option<String>,
    /// ツールバーの「メニューを PhotoCraft で編集」に描く PhotoCraft のアイコン（初めて描くときに読む）。
    pub(crate) photocraft_icon: Option<egui::TextureHandle>,
    /// 読んだ字幕ファイルの字幕（アセットの id ごと）。
    pub(crate) subtitle_cache: BTreeMap<u64, std::sync::Arc<Vec<crate::subtitles::SubtitleCue>>>,
    /// Picture subtitles drawn as on the disc, by what they are drawn from (see `image_subtitle_texture`).
    pub(crate) image_subtitles:
        std::collections::HashMap<u64, Option<(egui::TextureHandle, [f32; 4])>>,
    /// タイムライン ▸ 字幕を書き出し で選んだトラックのアセット（メニューを閉じてから保存する）。
    pub(crate) subtitle_export: Option<AssetId>,
    /// The macOS menu bar (M16.4: craft-menu's native menus, as the other Crafting Apps have), when installed; the
    /// menus are drawn in the window otherwise.
    pub(crate) native_menu: Option<craft_menu::NativeMenu>,
    /// モニターの拡大率。
    pub(crate) monitor_zoom: crate::monitor::MonitorZoom,
    /// メニュービューアの拡大率。
    pub(crate) menu_zoom: crate::monitor::MonitorZoom,
    /// ツールバーで選んでいるツール（選択 / テキスト / ズーム）。
    pub(crate) tool: crate::menu_text::Tool,
    /// メニュービューアで選んでいるテキスト（ビューアのメニューのテキストの番号）。
    pub(crate) selected_text: Option<usize>,
    /// ドラッグ中のテキスト。
    pub(crate) text_drag: Option<crate::menu_text::TextDrag>,
    /// メニュービューアで選んでいる画像 / シェイプ（番号）。
    pub(crate) selected_graphic: Option<usize>,
    /// ドラッグ中の画像 / シェイプ。
    pub(crate) graphic_drag: Option<crate::menu_graphic::GraphicDrag>,
    /// ダイレクト選択ツールで選んでいる、選んでいるボタンの中の部品（文字 / ビデオサムネール）。
    pub(crate) selected_part: Option<crate::menu_tools::ButtonPart>,
    /// ダイレクト選択ツールでドラッグ中の部品。
    pub(crate) part_drag: Option<crate::menu_tools::PartDrag>,
    /// 回転ツールで回しているテキスト / 画像。
    pub(crate) rotate_drag: Option<crate::menu_tools::RotateDrag>,
    /// 最後に押したパネル（Encore と同じく、Delete キーなどはこのパネルの選択に効く）。
    pub(crate) active_panel: Option<crate::workspace::Panel>,
    /// レイヤーパネルで閉じているボタンセット（メニューとボタンの番号）。初めは開いている（Encore と同じ）。
    pub(crate) collapsed_layers: std::collections::HashSet<(MenuId, usize)>,
    /// レイヤーパネルで、初めの開き方から開け閉めしたページのフォルダー（`(#) ページ n`）。初めは、ビューアに
    /// 出ているページのフォルダーだけが開いている。
    pub(crate) toggled_pages: std::collections::HashSet<MenuId>,
    /// M10.47: PSD から作ったメニューのレイヤーパネルに出す PSD のレイヤーの木（ファイルのパスごと、更新の時刻つき）。
    pub(crate) psd_trees: std::collections::HashMap<
        String,
        (
            Option<std::time::SystemTime>,
            std::sync::Arc<Vec<disccraft_menu::PsdTreeNode>>,
        ),
    >,
    /// M10.47: PSD のレイヤーの木で、人が開け閉めしたグループ（ファイルのパスと、上からの番号の道）と、開いているか。
    /// 表示するページが変わってもそのまま（M10.48）。
    pub(crate) psd_groups_open: std::collections::HashMap<(String, Vec<usize>), bool>,
    /// ドラッグ中のボタンのルート（ルートを手で決めるとき）。
    pub(crate) route_drag: Option<crate::route_drag::RouteDrag>,
    /// 開いている「ユーザー操作」ダイアログ。
    pub(crate) user_ops_dialog: Option<crate::user_ops::UserOpsDialog>,
    /// 開いている「再生リストの名前」ダイアログ（Encore の 新規 ▸ 再生リスト…）の入力中の名前。
    pub(crate) playlist_name_dialog: Option<String>,
    /// The Transcode Settings dialog, while it is open.
    pub(crate) transcode_settings_dialog: Option<crate::transcode_settings::TranscodeDraft>,
    /// The user's quality presets (name, kbit/s), and the Edit Quality Presets dialog's copy while it is open.
    pub(crate) quality_presets: Vec<crate::workspace::QualityPreset>,
    pub(crate) quality_presets_dialog: Option<Vec<crate::transcode_settings::PresetRow>>,
    /// Timeline ▸ Edit Timeline Color Set…: the timeline whose dialog is open.
    pub(crate) timeline_color_set_dialog: Option<TimelineId>,
    /// Timeline ▸ Import Subtitles ▸ Image Script / FAB Image Script: the script being imported (Map Colors).
    pub(crate) image_script_import: Option<crate::image_script::ImageScriptImport>,
    /// Where the video goes out (Preferences ▸ Audio/Video Output), and whether it stays up in the background.
    pub(crate) video_output: crate::video_out::VideoOutput,
    pub(crate) video_output_in_background: bool,
    /// Window ▸ Tools: the toolbar under the menu bar is shown (M6.339).
    pub(crate) show_tools: bool,
    /// Preferences ▸ General ▸ Playback quality: how large the Monitor plays a title's pictures (M6.341).
    pub(crate) playback_quality: crate::workspace::PlaybackQuality,
    /// The picture the Monitor showed of a title this frame: what goes out.
    pub(crate) video_out_frame: Option<crate::video_out::VideoFrame>,
    /// The Styles panel: the set shown, the style selected, the user's styles.
    pub(crate) styles: crate::styles_panel::StylesState,
    /// The XMP metadata read of assets' files, for the Metadata panel (by asset).
    pub(crate) metadata_cache:
        std::collections::HashMap<u64, std::sync::Arc<disccraft_engine::metadata::Metadata>>,
    /// What the Metadata panel's search field holds.
    pub(crate) metadata_search: String,
    /// M6.396 / M6.397: the Metadata panel's editable properties of the asset shown: the asset, the values read and
    /// the values being edited, by key (`dc:title`).
    pub(crate) metadata_draft: Option<(
        u64,
        std::collections::HashMap<String, String>,
        std::collections::HashMap<String, String>,
    )>,
    /// The Interpret Footage dialog, while it is open.
    pub(crate) interpret_footage_dialog: Option<crate::interpret_footage::InterpretDraft>,
    /// File ▸ Import ▸ Blu-ray…: the disc's playlists to choose from (M6.429).
    pub(crate) bluray_dialog: Option<crate::bluray_import::BlurayDraft>,
    /// 開いている「リンクの指定」ダイアログ（Ctrl+L）。
    pub(crate) link_dialog: Option<crate::link_dialog::LinkDialog>,
    /// The Drop Shadow dialog while it is open.
    pub(crate) shadow_dialog: Option<crate::shadow_dialog::ShadowDialog>,
    /// The rendered motion menu the project preview is playing (M6.334).
    pub(crate) preview_motion: Option<crate::project_preview::MotionPlay>,
    /// The Layer Effects dialog of an image or a shape, while it is open (M6.332).
    pub(crate) effects_dialog: Option<crate::effects_dialog::EffectsDialog>,
    /// The Menu Color Set dialog while it is open.
    pub(crate) color_set_dialog: Option<crate::color_set_dialog::ColorSetDialog>,
    /// プロジェクトパネルの概要に出すメニューのサムネール（メニューの id → メニューのハッシュとテクスチャ）。
    pub(crate) menu_thumbnails: std::collections::HashMap<u64, (u64, Option<egui::TextureHandle>)>,
    /// ビデオのアセットの最初のフレーム（モーションメニューの背景を絵で見せる。アセットの id → RGB と大きさ）。
    /// このコンピューターのフォントのファミリー（フォントの一覧に使う。初めて使うときに読む）。
    pub(crate) font_families: Option<std::sync::Arc<Vec<String>>>,
    /// PhotoCraft で編集中のメニューの PSD（保存されたらメニューに反映する）。
    pub(crate) external_edits: Vec<crate::photocraft::ExternalEdit>,
    /// 最後に PSD を調べた時刻（秒。1 秒ごとに調べる）。
    pub(crate) external_check: f64,
    /// リンクの欄の ▶ ▸ コピー で覚えたリンク。
    pub(crate) link_clipboard: Option<disccraft_project::Link>,
    /// プロジェクトパネルに出す項目の種類（Encore の左下の絞り込み）。
    pub(crate) project_filter: crate::project_panel::ProjectFilter,
    /// プロジェクトの表で隠している列（`ProjectColumn::ALL` の番号）。
    pub(crate) hidden_project_columns: Vec<usize>,
    /// 表の列の幅（見出しの右端をドラッグして決めたもの。設定に保存する）。
    pub(crate) column_widths: crate::theme::ColumnWidths,
    /// 列の幅が変わって、まだ設定に保存していない。
    pub(crate) column_widths_dirty: bool,
    /// プロジェクトの表で畳んでいるフォルダー。
    pub(crate) collapsed_folders: Vec<u64>,
    /// 名前を入力中のフォルダーと、その入力。
    pub(crate) renaming_folder: Option<(u64, String)>,
    /// 新規 ▸ で作る項目を入れるフォルダー（フォルダーの行の右クリックから作っているあいだだけ）。
    pub(crate) new_item_folder: Option<u64>,
    /// フローチャートのノードにサムネールを出す（Encore の「すべてのサムネールを表示」）。
    pub(crate) flow_thumbnails: bool,
    /// フローチャートでボタンを隠しているメニュー（ノードの右クリックの「ボタンを表示」を外したもの）。
    pub(crate) flow_hidden_buttons: Vec<MenuId>,
    /// フローチャートの拡大率（Encore のフローチャートとトレイの間のスライダー。1 で Encore のいつもの大きさ）。
    pub(crate) flow_zoom: f32,
    /// フローチャートのノードに出すチャプターの絵（タイムライン、チャプター、その時刻、クリップの並びごと。M6.432、
    /// 見えなくなったものは捨てる M6.436）。
    pub(crate) flow_frames:
        std::collections::BTreeMap<(u64, u32, i64, u64), Option<egui::TextureHandle>>,
    /// フローチャートで親ごとに揃えるリンク（Encore: 最後にクリックしたボタンの行か子の箱のリンク。選択を外しても残り、
    /// 開き直すと既定の並び。M6.440）。
    pub(crate) flow_focus:
        std::collections::BTreeMap<crate::flowchart::NodeId, crate::flowchart::Port>,
    /// リンクを揃え直したクリックの的（次の描画で画面の同じ位置に留まるようスクロールする）。
    pub(crate) flow_anchor: Option<crate::flowchart::FlowAnchor>,
    /// フローチャートでクリックした場所（項目そのものか、その繰り返しか。Encore はそこだけ青く光らせ、ほかは灰色）。
    pub(crate) flow_clicked: Option<crate::flowchart::AnchorItem>,
    /// フローチャートの箱の絵になるチャプターのポスター（タイムラインはチャプター 1、チャプター再生リストは最初の項目。
    /// M6.439、鍵は `flow_frames` と同じ）。
    pub(crate) flow_node_frames:
        std::collections::BTreeMap<(u64, u32, i64, u64), Option<egui::TextureHandle>>,
    /// メニュービューアでピクセルをテレビ画面の縦横比に補正する（Encore の「テレビ画面に合わせてメニュー
    /// ピクセルを校正」。外すと 720x480 をそのままの比率で出す）。
    pub(crate) correct_pixel_aspect: bool,
    /// 表ごとの並べ替え（表の id → 列と向き）。
    pub(crate) table_sorts: std::collections::HashMap<&'static str, crate::theme::TableSort>,
    /// The first frame of each motion menu's video (asset, for a 16:9 menu: M6.406).
    pub(crate) first_frames:
        std::collections::HashMap<(u64, bool), Option<(std::sync::Arc<Vec<u8>>, usize, usize)>>,
    /// Frames of videos at a time (asset, ticks, for a video thumbnail: whether the first bright frame from there),
    /// for a pop-up menu's background and the video thumbnails in the editor.
    pub(crate) timed_frames: std::collections::HashMap<
        (u64, i64, Option<bool>),
        Option<(std::sync::Arc<Vec<u8>>, usize, usize)>,
    >,
    /// モニターに出す字幕トラック（タイムラインごと）。Encore の字幕トラックの見出しの切り替えと同じく 0 本か 1 本で、
    /// 表示の状態なので保存も取り消しもしない。ないときは 1 本目。
    pub(crate) shown_subtitles: std::collections::HashMap<TimelineId, Option<usize>>,
    /// モニターで聞くオーディオトラック（タイムライン → 選んだときのトラック数と、選んだトラック）。
    pub(crate) heard_audio: std::collections::HashMap<TimelineId, (usize, usize)>,
    /// モニターで入力中の字幕（字幕を追加したあとと、モニターの字幕をダブルクリックしたとき）。
    pub(crate) subtitle_edit: Option<crate::subtitle_edit::SubtitleEdit>,
    /// タイムラインの再生（タイムライン ▸ 再生、Space）。
    pub(crate) playback: Option<crate::playback::Playback>,
    /// How fast the Monitor plays a timeline (M6.427): 1 real time, 2 twice as fast. From the speed menu under
    /// the Monitor and J / K / L.
    pub(crate) playback_speed: f64,
    /// タイムラインのビデオクリップの位置（タイムライン、クリップとトリムのキー、クリップ）。
    pub(crate) clip_cache: Option<(TimelineId, Vec<u64>, Vec<crate::playback::ClipSpan>)>,
    /// Why an asset's video is converted for the project's Blu-ray (M6.376), by asset and Blu-ray format: read from
    /// the file once.
    pub(crate) bd_mismatch_cache: std::collections::HashMap<
        (u64, disccraft_project::BdVideo),
        Option<disccraft_build::BdMismatch>,
    >,
    /// ビルド結果のプレビューで再生しているタイトル。
    pub(crate) preview_playback: Option<crate::preview_playback::PreviewPlayback>,
    /// プロジェクトプレビューの窓（開いているときだけ）。
    pub(crate) project_preview: Option<crate::project_preview::ProjectPreview>,
    /// プレビューのタイトルの再生を一時停止している。
    pub(crate) preview_paused: bool,
    /// A Random or Shuffle chapter playlist's play order in the preview (its title, and the entries still to
    /// play, the one playing first), kept while it plays.
    pub(crate) preview_order: Option<(u8, Vec<u16>)>,
    /// 次にプレビューのタイトルを再生し始める位置（秒。一時停止からの再生、オーディオ / 字幕の切り替え）。
    pub(crate) preview_resume: Option<f64>,
    /// The slides' pictures shown in the preview, by asset (larger than the project's thumbnails).
    pub(crate) preview_slides: std::collections::HashMap<u64, Option<egui::TextureHandle>>,
    pub(crate) menu_texture: Option<MenuTexture>,
    /// プロジェクトプレビューで再生中のタイトルに重ねる Blu-ray のポップアップメニューの絵（透明度つき）。
    pub(crate) popup_texture: Option<crate::project_preview::PopupTexture>,
    /// メニュービューアで選んでいるボタン（選んだ順。最後が主で、プロパティはそのシートを出す）。
    pub(crate) selected_buttons: Vec<usize>,
    /// 編集 ▸ コピー / 切り取りしたボタン。
    pub(crate) button_clipboard: Vec<crate::button_edit::ClipButton>,
    /// The graphic layer last copied or cut (Edit ▸ Paste as Subpicture makes it a button's subpicture).
    pub(crate) graphic_clipboard: Option<disccraft_project::MenuGraphic>,
    /// このフレームで Ctrl+V されたときのシステムのクリップボードの文字列。
    pub(crate) pasted_text: Option<String>,
    /// オブジェクト ▸ 整列 ▸ セーフエリアに相対的に整列。
    pub(crate) align_to_safe_area: bool,
    /// オブジェクト ▸ 均等配置 ▸ セーフエリアに相対的に均等配置。
    pub(crate) distribute_to_safe_area: bool,
    /// キャンバスの何もないところからのドラッグ（範囲で選ぶ）。
    pub(crate) marquee: Option<crate::menu_editor::Marquee>,
    pub(crate) button_label: String,
    pub(crate) button_label_menu: Option<MenuId>,
    pub(crate) button_label_button: Option<usize>,
    pub(crate) bg_color: [f32; 3],
    pub(crate) color_menu: Option<MenuId>,
    pub(crate) rename_key: Option<(&'static str, u64)>,
    pub(crate) rename_value: String,
    pub(crate) slide_asset: Option<AssetId>,
    /// スライドショービューアで選んだスライド（スライドショーの id, 番号）。プロパティはスライドのシートになる。
    pub(crate) slide_focus: Option<(u64, usize)>,
    /// 再生リストのシートで選んだ項目（再生リストの id, 番号）。
    pub(crate) playlist_item_pick: Option<(u64, usize)>,
    pub(crate) canvas_drag: Option<CanvasDrag>,
    /// ドラッグ中のボタンの今の矩形（番号と矩形）。
    pub(crate) drag_rects: Vec<(usize, Rect)>,
    /// メニュービューアにセーフエリアを重ねるか（Encore のバーのボタン。M6.489 から初めは出さない）。
    pub(crate) show_safe_areas: bool,
    /// モニターにセーフエリアを重ねるか（Encore のモニターのボタン。初めは出さない）。
    pub(crate) show_monitor_safe_areas: bool,
    pub(crate) show_routing: bool,
    /// 表示 ▸ ガイドを表示 / ガイドにスナップ / ガイドをロック（ガイドを表示は M6.489 から初めはオフ。新規ガイドを
    /// 打つとオンになる）。
    pub(crate) show_guides: bool,
    pub(crate) snap_to_guides: bool,
    pub(crate) lock_guides: bool,
    /// 「新規ガイド」ダイアログ。
    pub(crate) guide_dialog: Option<crate::guides::GuideDialog>,
    /// ドラッグ中のガイド。
    pub(crate) guide_drag: Option<crate::guides::GuideDrag>,
    /// タイムラインのチャプターポイントの横に名前を出すか（表示 ▸ チャプター名を表示）。
    pub(crate) show_chapter_names: bool,
    /// タイムライン ▸ オーディオトラックを追加… / 字幕トラックを追加… のダイアログ。
    pub(crate) track_dialog: Option<crate::track_dialog::TrackDialog>,
    /// ヘルプ ▸ DiscCraft について… のダイアログを開いているか。
    pub(crate) about_open: bool,
    /// ヘルプ ▸ システム互換性レポート… が開いている。
    pub(crate) system_report_open: bool,
    /// ヘルプ ▸ アップデートを確認… が開いている。
    pub(crate) update_open: bool,
    /// ログに書いた最後のステータス（同じ文を何度も書かない）。
    pub(crate) logged_status: String,
    /// タイムラインビューアで選んだチャプターポイント。プロパティパネルはこのときチャプターのシートを出す。
    pub(crate) chapter_focus: Option<(TimelineId, usize)>,
    /// タイムラインビューアで選んだ字幕（タイムライン、字幕トラック、字幕の番号）。プロパティはそのシートになる。
    pub(crate) cue_focus: Option<(TimelineId, usize, usize)>,
    /// チャプターのシートの入力欄が表しているチャプター（タイムラインの id, 番号）。
    pub(crate) chapter_key: Option<(u64, usize)>,
    /// チャプターのシートの名前の入力欄。
    pub(crate) chapter_name_value: String,
    /// チャプターのシートのインの入力欄。
    pub(crate) chapter_in_value: String,
    /// チャプターのシートのポスターフレームの入力欄。
    pub(crate) chapter_poster_value: String,
    /// メニュービューアで全部のボタンに重ねるサブピクチャの状態（Encore の 標準 / 選択時 / アクティベート時。
    /// M6.489: 初めはどれでもない。1 つずつで、押しているものをもう一度押すと消える）。
    pub(crate) highlight_preview: Option<HighlightPreview>,
    pub(crate) preview: PreviewState,
    pub(crate) build_output: OutputKind,
    pub(crate) build_path: String,
    pub(crate) build_job: BuildJob,
    /// 「今すぐトランスコード」の裏の仕事（ビルドと同じキャッシュに変換だけを作る）。
    pub(crate) transcode_job: BuildJob,
    pub(crate) last_build: Option<BuildReport>,
    /// The user manual's screenshot script being played (M15.1, `DISCCRAFT_CAPTURE`).
    pub(crate) doc_capture: Option<crate::doc_capture::DocCapture>,
    pub(crate) last_output_kind: Option<OutputKind>,
    pub(crate) simulate: Option<SimulateSummary>,
    pub(crate) usage: Option<Usage>,
    pub(crate) problems: Vec<ProblemView>,
    /// A media file is still being opened on its own thread (a long file takes seconds): the estimate, the check
    /// and the transcode status wait for it instead of holding up the window.
    pub(crate) media_pending: bool,
    pub(crate) usage_dirty: bool,
    pub(crate) check_dirty: bool,
    /// アセットごとのトランスコードステータス（`asset.transcodeStatus`）。
    pub(crate) transcode: BTreeMap<u64, crate::project_panel::TranscodeState>,
    /// The assets the running build or Transcode Now has to transcode, as they were when it started: the Progress
    /// panel lists those not transcoded yet (Encore's list of the transcodes, M6.340).
    pub(crate) transcode_queue: Vec<u64>,
    /// When the Progress panel last had the transcode status looked at again while a job ran.
    pub(crate) transcode_queue_checked: Option<std::time::Instant>,
    /// Whether each video asset would fit a DVD and a Blu-ray as it is (`asset.transcodeStatus`'s "fitsDvd" and
    /// "fitsBluray"), whether or not a preset of its own transcodes it all the same.
    pub(crate) transcode_fits: BTreeMap<u64, (bool, bool)>,
    /// トランスコードステータスを問い合わせたときのビルドの出力先。変わったら問い合わせ直す。
    pub(crate) transcode_for: Option<(String, OutputKind)>,
    pub(crate) transcode_dirty: bool,
    /// The Build panel's disc output and the writing of discs (M9.35).
    pub(crate) disc: crate::disc_writer::DiscWriter,
    /// The Build Progress dialog's timing and the taskbar button (M6.505).
    pub(crate) progress_watch: crate::build_progress::ProgressWatch,
    /// File ▸ Create Disc Label…'s dialog while it is open (M6.506).
    pub(crate) disc_label: Option<crate::disc_label::DiscLabelDialog>,
}

impl DiscCraftApp {
    /// Creates a new application state.
    pub fn new(ctx: &Context) -> Self {
        // Preferences that are not the window's own: the audio output device, and how many transcodes run at
        // a time.
        crate::audio_out::choose_device(crate::workspace::load_audio_device());
        disccraft_build::set_parallel_transcodes(
            usize::try_from(crate::workspace::load_parallel_transcodes()).unwrap_or(0),
        );
        disccraft_platform::use_for_transcodes(crate::workspace::load_hardware_encoder());
        ctx.request_repaint();

        crate::help::log(&format!(
            "DiscCraft {} started on {} ({})",
            crate::help::version_text(),
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
        let (workspace, dock, user_workspaces) = load_workspace();
        let language = load_language();
        crate::i18n::set_current(language);

        Self {
            session: Session::new(),
            selection: Selection::None,
            project_clipboard: Selection::None,
            status: tl!("DiscCraft started").to_string(),
            language,
            workspace,
            dock,
            dock_dirty: false,
            maximized_panel: None,
            panel_bodies: Vec::new(),
            user_workspaces,
            new_workspace: None,
            preload_waveforms: false,
            focus_rename: false,
            view_request: None,
            last_menu: None,
            last_timeline: None,
            last_slideshow: None,
            last_chapter_playlist: None,
            chapter_playlist_pick: None,
            chapter_playlist_picks: Vec::new(),
            chapter_entry_pick: None,
            durations: BTreeMap::new(),
            thumbnails: BTreeMap::new(),
            last_frames: BTreeMap::new(),
            waveforms: crate::waveforms::Waveforms::default(),
            clip_thumbs: crate::clip_thumbs::ClipThumbs::default(),
            welcome: Welcome::opened(),
            recent: load_recent(),
            check_window: crate::check_window::CheckWindow::default(),
            pending_action: None,
            build_prompt: false,
            build_failure: None,
            film_marker_dialog: None,
            film_export: None,
            allow_close: false,
            prefs: crate::prefs::PrefsWindow::default(),
            timeline_prefs: crate::workspace::load_timeline_prefs(),
            preview_prefs: crate::workspace::load_preview_prefs(),
            library: crate::library_panel::LibraryState::default(),
            user_library: crate::user_library::UserLibrary::load(),
            clip_selection: None,
            next_text: serde_json::Map::new(),
            library_set_dialog: None,
            library_import: None,
            bluray_subtitles: None,
            library_previews: std::collections::HashMap::new(),
            chapter_interval: None,
            monitor_frame: None,
            monitor_loader: disccraft_engine::FrameLoader::new(),
            monitor_wanted: None,
            monitor_note: None,
            photocraft_icon: None,
            playback: None,
            playback_speed: 1.0,
            clip_cache: None,
            bd_mismatch_cache: std::collections::HashMap::new(),
            preview_playback: None,
            project_preview: None,
            preview_paused: false,
            preview_order: None,
            preview_resume: None,
            preview_slides: std::collections::HashMap::new(),
            monitor_zoom: crate::monitor::MonitorZoom::Fit,
            menu_zoom: crate::monitor::MonitorZoom::Fit,
            tool: crate::menu_text::Tool::Selection,
            selected_text: None,
            text_drag: None,
            selected_graphic: None,
            graphic_drag: None,
            selected_part: None,
            part_drag: None,
            rotate_drag: None,
            active_panel: None,
            collapsed_layers: std::collections::HashSet::new(),
            toggled_pages: std::collections::HashSet::new(),
            psd_trees: std::collections::HashMap::new(),
            psd_groups_open: std::collections::HashMap::new(),
            route_drag: None,
            user_ops_dialog: None,
            playlist_name_dialog: None,
            transcode_settings_dialog: None,
            quality_presets: crate::workspace::load_quality_presets(),
            quality_presets_dialog: None,
            timeline_color_set_dialog: None,
            image_script_import: None,
            interpret_footage_dialog: None,
            bluray_dialog: None,
            video_output: crate::video_out::VideoOutput::from_setting(
                crate::workspace::load_video_output().0,
            ),
            video_output_in_background: crate::workspace::load_video_output().1,
            show_tools: !crate::workspace::load_hide_tools(),
            playback_quality: crate::workspace::load_playback_quality(),
            video_out_frame: None,
            styles: crate::styles_panel::StylesState {
                user: crate::styles_panel::UserStyles::load(),
                ..Default::default()
            },
            metadata_cache: std::collections::HashMap::new(),
            metadata_search: String::new(),
            metadata_draft: None,
            link_dialog: None,
            shadow_dialog: None,
            preview_motion: None,
            effects_dialog: None,
            color_set_dialog: None,
            menu_thumbnails: std::collections::HashMap::new(),
            first_frames: std::collections::HashMap::new(),
            timed_frames: std::collections::HashMap::new(),
            shown_subtitles: std::collections::HashMap::new(),
            heard_audio: std::collections::HashMap::new(),
            subtitle_edit: None,
            font_families: None,
            external_edits: Vec::new(),
            external_check: 0.0,
            link_clipboard: None,
            table_sorts: std::collections::HashMap::new(),
            project_filter: crate::project_panel::ProjectFilter::default(),
            flow_thumbnails: true,
            collapsed_folders: Vec::new(),
            renaming_folder: None,
            new_item_folder: None,
            flow_hidden_buttons: Vec::new(),
            flow_zoom: 1.0,
            flow_frames: std::collections::BTreeMap::new(),
            flow_node_frames: std::collections::BTreeMap::new(),
            flow_focus: std::collections::BTreeMap::new(),
            flow_anchor: None,
            flow_clicked: None,
            correct_pixel_aspect: true,
            column_widths: crate::workspace::load_settings().column_widths,
            column_widths_dirty: false,
            hidden_project_columns: crate::project_panel::ProjectColumn::ALL
                .iter()
                .enumerate()
                .filter(|(_, column)| !column.shown_by_default())
                .map(|(index, _)| index)
                .collect(),
            subtitle_cache: BTreeMap::new(),
            image_subtitles: std::collections::HashMap::new(),
            subtitle_export: None,
            native_menu: None,
            menu_texture: None,
            popup_texture: None,
            selected_buttons: Vec::new(),
            button_clipboard: Vec::new(),
            graphic_clipboard: None,
            pasted_text: None,
            align_to_safe_area: false,
            distribute_to_safe_area: false,
            marquee: None,
            button_label: String::new(),
            button_label_menu: None,
            button_label_button: None,
            bg_color: [0.0, 0.0, 0.0],
            color_menu: None,
            rename_key: None,
            rename_value: String::new(),
            slide_asset: None,
            slide_focus: None,
            playlist_item_pick: None,
            canvas_drag: None,
            drag_rects: Vec::new(),
            show_safe_areas: false,
            show_monitor_safe_areas: false,
            show_routing: false,
            show_guides: false,
            snap_to_guides: true,
            lock_guides: false,
            guide_dialog: None,
            guide_drag: None,
            show_chapter_names: false,
            track_dialog: None,
            about_open: false,
            system_report_open: false,
            update_open: false,
            logged_status: String::new(),
            chapter_focus: None,
            cue_focus: None,
            chapter_key: None,
            chapter_name_value: String::new(),
            chapter_in_value: String::new(),
            chapter_poster_value: String::new(),
            highlight_preview: None,
            preview: PreviewState::default(),
            build_output: OutputKind::Folder,
            build_path: String::new(),
            build_job: BuildJob::default(),
            transcode_job: BuildJob::default(),
            last_build: None,
            doc_capture: crate::doc_capture::from_env(),
            last_output_kind: None,
            simulate: None,
            usage: None,
            problems: Vec::new(),
            media_pending: false,
            usage_dirty: true,
            transcode: BTreeMap::new(),
            transcode_queue: Vec::new(),
            transcode_queue_checked: None,
            transcode_fits: BTreeMap::new(),
            transcode_for: None,
            transcode_dirty: true,
            disc: crate::disc_writer::DiscWriter::default(),
            progress_watch: crate::build_progress::ProgressWatch::default(),
            disc_label: None,
            check_dirty: true,
        }
    }

    /// Opens a project file.
    pub fn open_path(&mut self, path: &Path) -> Result<(), String> {
        let session = Session::open(path)?;
        self.session = session;
        self.reset_state();
        self.preload_viewers();
        self.welcome = Welcome::default();
        self.status = tlf!("Opened: {path}", path = path.display());
        self.remember(path);
        Ok(())
    }

    /// Draws the whole UI.
    pub fn ui(&mut self, ui: &mut Ui) {
        // Strings are looked up per thread; set the language before anything is drawn. The words the project makes up
        // (default names, a menu's PSD and PhotoCraft layers) follow it too (M6.433), whatever project is open.
        crate::i18n::set_current(self.language);
        self.session
            .set_words(disccraft_project::Words::from_code(self.language.code()));
        // M15.1: the user manual's screenshot script, when one runs.
        self.play_capture(ui);

        if let Some(result) = self.build_job.poll() {
            self.handle_build_result(result);
        }
        self.poll_writing(ui.ctx());
        // M6.505: while the Build Progress dialog is up, no key reaches the window under it.
        self.hold_keys(ui.ctx());

        if self.build_job.is_running() {
            ui.ctx().request_repaint();
        }
        self.poll_transcode_now(ui.ctx());
        self.poll_bluray_subtitles(ui.ctx());

        self.validate_selection();

        let target = (self.build_path.trim().to_owned(), self.build_output);
        let transcode_due = self.transcode_dirty || self.transcode_for.as_ref() != Some(&target);
        if (self.usage_dirty || self.check_dirty || transcode_due || self.media_pending)
            && !self.wait_for_media(ui.ctx())
        {
            if self.usage_dirty {
                self.refresh_usage();
            }

            if self.check_dirty {
                self.refresh_check();
            }

            if transcode_due {
                self.refresh_transcode();
            }
        }

        // The window's ground under the menu bar, in the look on screen (M6.480).
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, crate::theme::APP_BG);
        self.menu_bar(ui);
        self.start_preloading_waveforms(ui.ctx());
        self.poll_playback(ui.ctx());
        self.poll_preview_playback(ui.ctx());
        // The timeline's pictures wait while something plays (they made the monitor stutter).
        let playing =
            self.is_playing() || self.preview_playback.is_some() || self.project_preview.is_some();
        self.clip_thumbs.set_paused(playing);
        self.poll_external_edits(ui.ctx());
        self.draw_groups(ui);
        // Files dropped on the window while a build runs are not imported (the window waits for the build).
        if !self.job_running() {
            self.accept_dropped_files(ui.ctx());
        }
        // パネルが矢印キーを使わなかったときだけ（メニュービューアのボタンを動かすのが先）。
        self.step_frames_by_keys(ui.ctx());
        self.check_window(ui.ctx());
        self.prefs_window(ui.ctx());
        self.chapter_interval_dialog(ui.ctx());
        self.disc_label_window(ui.ctx());
        self.new_workspace_dialog(ui.ctx());
        self.project_preview_window(ui.ctx());
        self.track_dialog_window(ui.ctx());
        self.guide_dialog_window(ui.ctx());
        self.user_ops_window(ui.ctx());
        self.playlist_name_window(ui.ctx());
        self.transcode_settings_window(ui.ctx());
        self.quality_presets_window(ui.ctx());
        self.library_set_window(ui.ctx());
        self.timeline_color_set_window(ui.ctx());
        self.image_script_window(ui.ctx());
        self.video_output_window(ui.ctx());
        // The next frame's picture is whatever the Monitor shows then.
        self.video_out_frame = None;
        self.interpret_footage_window(ui.ctx());
        self.bluray_import_window(ui.ctx());
        self.link_window(ui.ctx());
        self.shadow_window(ui.ctx());
        self.effects_window(ui.ctx());
        self.color_set_window(ui.ctx());
        self.about_window(ui.ctx());
        self.system_report_window(ui.ctx());
        self.update_window(ui.ctx());
        self.show_welcome(ui);
        self.show_confirm(ui.ctx());
        self.show_build_prompt(ui.ctx());
        self.show_build_failure(ui.ctx());
        self.show_writer_prompt(ui.ctx());
        self.show_build_progress(ui.ctx());
        self.show_film_marker_dialog(ui.ctx());
        self.poll_film_export(ui.ctx());
        self.intercept_close(ui.ctx());
        // M6.518: the pick whip's tether goes over every panel and window, so it is drawn last.
        let _ = crate::pickwhip::draw_tether(ui.ctx());
        // What the status bar said, for the session log (Help ▸ Reveal Log Files…).
        if self.status != self.logged_status {
            crate::help::log(&format!("status: {}", self.status));
            self.logged_status.clone_from(&self.status);
        }
    }

    /// ようこそ画面と新規プロジェクトダイアログを描き、選ばれたことを実行する。
    fn show_welcome(&mut self, ui: &Ui) {
        let Some(choice) = self.welcome.show(ui.ctx(), &self.recent) else {
            return;
        };

        use crate::confirm::PendingAction;
        match choice {
            WelcomeChoice::NewProject(form) => {
                self.confirm_or_run(ui.ctx(), PendingAction::NewProject(form));
            }
            WelcomeChoice::ProjectSettings(form) => self.apply_project_settings(&form),
            WelcomeChoice::Open => self.confirm_or_run(ui.ctx(), PendingAction::OpenProject),
            WelcomeChoice::OpenRecent(path) => {
                self.confirm_or_run(ui.ctx(), PendingAction::OpenRecent(path));
            }
            WelcomeChoice::Exit => self.confirm_or_run(ui.ctx(), PendingAction::Exit),
            WelcomeChoice::Forget(path) => match crate::workspace::forget_recent(&path) {
                Ok(recent) => self.recent = recent,
                Err(_) => self.recent.retain(|existing| existing != &path),
            },
        }
    }

    /// 新規プロジェクトダイアログの結果でプロジェクトを作り直す。
    pub(crate) fn apply_new_project(&mut self, form: &crate::new_project::NewProjectForm) {
        self.new_project();
        self.set_disc_kind(form.kind);
        self.set_disc_standard(form.standard);
        // A Blu-ray starts in the format picked in the dialog (HD unless SD was chosen, M6.357).
        if form.kind == DiscKind::BluRay {
            let _ = self.run_command(
                "disc.setBlurayVideo",
                json!({ "video": form.bd_video.id() }),
            );
            // As Encore's New Project dialog (M10.43: a PAL disc's 1080i 25 in H.264).
            let codec = form.bd_codec();
            if !codec.is_auto() {
                let _ = self.run_command("disc.setBlurayCodec", json!({ "codec": codec.id() }));
            }
        }
        if form.wide {
            let _ = self.run_command("disc.setAspect", json!({ "aspect": "16:9" }));
        }
        self.set_capacity(form.capacity);
        if let Some(path) = form.project_path() {
            self.save_to(&path);
        }
    }

    /// プロジェクト設定ダイアログの結果を反映する（変わったものだけコマンドを送る）。
    fn apply_project_settings(&mut self, form: &crate::new_project::NewProjectForm) {
        let settings = self.session.project.settings.clone();
        if form.kind != settings.kind {
            self.set_disc_kind(form.kind);
        }
        if form.kind == DiscKind::BluRay && form.bd_video != settings.bd_video {
            let _ = self.run_command(
                "disc.setBlurayVideo",
                json!({ "video": form.bd_video.id() }),
            );
        }
        if form.wide != settings.widescreen {
            let aspect = if form.wide { "16:9" } else { "4:3" };
            let _ = self.run_command("disc.setAspect", json!({ "aspect": aspect }));
        }
        if form.capacity != self.session.project.settings.capacity {
            self.set_capacity(form.capacity);
        }
    }

    /// プロジェクト設定ダイアログを開く。
    pub(crate) fn open_project_settings(&mut self) {
        let form = crate::new_project::NewProjectForm::for_project(
            self.session.path.as_deref(),
            &self.session.project.settings,
        );
        self.welcome.start_project_settings(form);
    }

    /// プロジェクトを閉じて、ようこそ画面に戻る（Encore と同じ）。
    pub(crate) fn close_project(&mut self) {
        self.new_project();
        self.welcome = Welcome::opened();
    }

    /// 最近使ったプロジェクトに足す（保存できなくても編集は止めない）。
    fn remember(&mut self, path: &Path) {
        match crate::workspace::remember_recent(path) {
            Ok(recent) => self.recent = recent,
            Err(_) => {
                crate::welcome::add_recent(&mut self.recent, path, crate::workspace::RECENT_LIMIT)
            }
        }
    }

    /// 表示言語を切り替えて設定に保存する（次のフレームから新しい言語で描く）。
    pub(crate) fn set_language(&mut self, language: Language) {
        self.language = language;
        crate::i18n::set_current(language);
        self.status = tlf!("Language: {name}", name = language.name());

        if let Err(error) = crate::workspace::save_language(language) {
            self.status = tlf!(
                "Language: {name} (could not save: {error})",
                name = language.name(),
                error
            );
        }
    }

    /// ワークスペースを切り替える。今のワークスペースの並びを覚えてから、切り替え先の覚えている並び（なければ
    /// その初期の並び）にし、設定に保存する。
    pub(crate) fn set_workspace(&mut self, id: WorkspaceId) {
        let mut result =
            crate::workspace::save_workspace(&self.workspace, &self.dock, &self.user_workspaces);
        let settings = crate::workspace::load_settings();
        self.dock = crate::workspace::saved_dock(&settings, &id)
            .unwrap_or_else(|| crate::workspace::default_dock(&settings, &id));
        self.workspace = id;
        if result.is_ok() {
            result = crate::workspace::save_workspace(
                &self.workspace,
                &self.dock,
                &self.user_workspaces,
            );
        }
        self.dock_dirty = false;
        self.workspace_status(result);
    }

    /// ワークスペースをリセットする: 名前を付けたものは保存したときの並び、組み込みのものはその初期の並びへ。
    pub(crate) fn reset_workspace(&mut self) {
        let default = crate::workspace::reset_layout(&self.workspace);
        self.dock.reset(&default);
        let result =
            crate::workspace::save_workspace(&self.workspace, &self.dock, &self.user_workspaces);
        self.dock_dirty = false;
        self.workspace_status(result);
    }

    /// 今の並びに名前を付けて保存し、そのワークスペースに切り替える。同じ名前があれば置き換える。
    pub(crate) fn save_new_workspace(&mut self, name: &str) {
        let name: String = name
            .trim()
            .chars()
            .take(crate::workspace::WORKSPACE_NAME_LIMIT)
            .collect();
        if name.is_empty() {
            return;
        }
        if !self.user_workspaces.contains(&name) {
            if self.user_workspaces.len() >= crate::workspace::USER_WORKSPACE_LIMIT {
                self.status = tl!("Too many workspaces; delete one first.").to_owned();
                return;
            }
            self.user_workspaces.push(name.clone());
        }
        // 離れるワークスペースの並びも覚えておく。
        let mut result =
            crate::workspace::save_workspace(&self.workspace, &self.dock, &self.user_workspaces);
        let mut dock = self.dock.clone();
        dock.saved = Some(dock.layout.clone());
        self.dock = dock;
        self.workspace = WorkspaceId::User(name);
        if result.is_ok() {
            result = crate::workspace::save_workspace(
                &self.workspace,
                &self.dock,
                &self.user_workspaces,
            );
        }
        self.dock_dirty = false;
        self.workspace_status(result);
    }

    /// 名前を付けたワークスペースを消す。使っていたら初期設定のワークスペースに戻る。
    pub(crate) fn delete_workspace(&mut self, name: &str) {
        self.user_workspaces.retain(|other| other != name);
        if self.workspace == WorkspaceId::User(name.to_owned()) {
            self.workspace = WorkspaceId::default();
            let settings = crate::workspace::load_settings();
            self.dock = crate::workspace::saved_dock(&settings, &self.workspace)
                .unwrap_or_else(|| crate::workspace::default_dock(&settings, &self.workspace));
        }
        let result =
            crate::workspace::save_workspace(&self.workspace, &self.dock, &self.user_workspaces);
        self.dock_dirty = false;
        self.workspace_status(result);
    }

    fn workspace_status(&mut self, result: Result<(), String>) {
        let name = self.workspace.label();
        self.status = match result {
            Ok(()) => tlf!("Workspace: {name}", name = name),
            Err(error) => tlf!(
                "Workspace: {name} (save failed: {error})",
                name = name,
                error = error
            ),
        };
    }

    /// 「新規ワークスペース…」ダイアログ（開いているときだけ描く）。
    fn new_workspace_dialog(&mut self, ctx: &Context) {
        let Some(mut name) = self.new_workspace.take() else {
            return;
        };
        let mut done = None;
        let response = egui::Modal::new(egui::Id::unique("new_workspace")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.heading(tl!("New Workspace"));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(tl!("Name:"));
                let field = ui.text_edit_singleline(&mut name);
                if !field.has_focus() && !field.lost_focus() {
                    field.request_focus();
                }
                if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
            });
            if self.user_workspaces.contains(&name.trim().to_owned()) {
                ui.small(tl!("A workspace with this name is replaced."));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !name.trim().is_empty(),
                        crate::theme::default_button(tl!("OK")),
                    )
                    .clicked()
                {
                    done = Some(true);
                }
                if ui.add(crate::theme::dialog_button(tl!("Cancel"))).clicked() {
                    done = Some(false);
                }
            });
        });
        if response.should_close() {
            done = Some(false);
        }
        match done {
            Some(true) => self.save_new_workspace(&name),
            Some(false) => {}
            None => self.new_workspace = Some(name),
        }
    }

    pub(crate) fn new_project(&mut self) {
        self.session = Session::new();
        self.reset_state();
        self.status = tl!("Created a new project").to_string();
    }

    pub(crate) fn open_project(&mut self) {
        if let Some(path) = FileDialog::new()
            .add_filter(tl!("DiscCraft project"), &["dcproj"])
            .pick_file()
        {
            let _ = self.open_path(&path);
        }
    }

    pub(crate) fn save_project(&mut self, save_as: bool) {
        let chosen = if save_as || self.session.path.is_none() {
            match FileDialog::new()
                .add_filter(tl!("DiscCraft project"), &["dcproj"])
                .save_file()
            {
                Some(path) => path,
                None => {
                    self.status = tl!("Save cancelled").to_string();
                    return;
                }
            }
        } else {
            self.session.path.clone().unwrap_or_default()
        };

        let result = if save_as {
            self.session.save(Some(&chosen))
        } else {
            self.session.save(None)
        };

        match result {
            Ok(saved) => {
                self.status = tlf!("Saved: {path}", path = saved.display());
                self.remember(&saved);
            }
            Err(error) => self.status = error,
        }
    }

    /// 決まった場所に保存する（新規プロジェクトダイアログで場所を指定したとき）。
    fn save_to(&mut self, path: &Path) {
        if let Some(dir) = path.parent()
            && let Err(error) = std::fs::create_dir_all(dir)
        {
            self.status = error.to_string();
            return;
        }
        match self.session.save(Some(path)) {
            Ok(saved) => {
                self.status = tlf!("Saved: {path}", path = saved.display());
                self.remember(&saved);
            }
            Err(error) => self.status = error,
        }
    }

    pub(crate) fn undo(&mut self) {
        if find_command("edit.undo").is_some() {
            self.run_command("edit.undo", json!({}));
            return;
        }

        match self.session.undo() {
            Ok(()) => {
                self.subtitle_cache.clear();
                self.status = tl!("Undone").to_string();
                self.mark_dirty();
            }
            Err(error) => self.status = error,
        }
    }

    pub(crate) fn redo(&mut self) {
        if find_command("edit.redo").is_some() {
            self.run_command("edit.redo", json!({}));
            return;
        }

        match self.session.redo() {
            Ok(()) => {
                self.subtitle_cache.clear();
                self.status = tl!("Redone").to_string();
                self.mark_dirty();
            }
            Err(error) => self.status = error,
        }
    }

    /// 開いたプロジェクトのタイムラインを、選ばずにタイムラインビューアへ読み込んでおく（スライドショーも
    /// スライドショービューアへ）。ビューアに出たタイムラインは、その映像のフレームとモニターの映像を作り始め、
    /// 次のフレームでほかのタイムラインのオーディオの波形も裏で作り始める（切り替えたときにすぐ出る）。
    fn preload_viewers(&mut self) {
        let project = &self.session.project;
        self.last_timeline = preload_timeline(project);
        self.last_slideshow = project.slideshows.keys().next().copied();
        self.preload_waveforms = true;
    }

    /// プロジェクトのすべてのタイムラインのオーディオトラックの波形を、裏で作り始める（開いた直後に 1 回）。
    fn start_preloading_waveforms(&mut self, ctx: &Context) {
        if !std::mem::take(&mut self.preload_waveforms) {
            return;
        }
        let assets: Vec<(u64, PathBuf)> = self
            .session
            .project
            .timelines
            .values()
            .flat_map(|timeline| timeline.audio.iter().map(|track| track.asset))
            .filter_map(|asset| {
                self.session
                    .project
                    .assets
                    .get(&asset)
                    .map(|found| (asset.0, PathBuf::from(&found.path)))
            })
            .collect();
        for (asset, path) in assets {
            let _ = self.waveforms.get(asset, path, ctx);
        }
    }

    fn reset_state(&mut self) {
        self.bluray_subtitles = None;
        self.selection = Selection::None;
        self.project_clipboard = Selection::None;
        self.last_menu = None;
        self.last_timeline = None;
        self.last_slideshow = None;
        self.last_chapter_playlist = None;
        self.chapter_playlist_pick = None;
        self.chapter_playlist_picks.clear();
        self.chapter_entry_pick = None;
        self.durations.clear();
        self.thumbnails.clear();
        self.last_frames.clear();
        self.waveforms.clear();
        self.clip_thumbs.clear();
        self.monitor_frame = None;
        self.playback = None;
        self.preview_playback = None;
        self.preview_slides.clear();
        self.flow_frames.clear();
        self.flow_node_frames.clear();
        self.flow_focus.clear();
        self.flow_anchor = None;
        self.flow_clicked = None;
        self.metadata_cache.clear();
        self.subtitle_cache.clear();
        self.image_subtitles.clear();
        self.subtitle_edit = None;
        self.selected_buttons.clear();
        self.button_label_menu = None;
        self.button_label_button = None;
        self.button_label.clear();
        self.color_menu = None;
        self.menu_texture = None;
        self.rename_key = None;
        self.rename_value.clear();
        self.chapter_focus = None;
        self.chapter_key = None;
        self.slide_asset = None;
        self.slide_focus = None;
        self.playlist_item_pick = None;
        self.canvas_drag = None;
        self.drag_rects.clear();
        self.marquee = None;
        self.build_path.clear();
        self.build_job = BuildJob::default();
        self.last_build = None;
        self.last_output_kind = None;
        self.simulate = None;
        self.usage = None;
        self.problems.clear();
        self.usage_dirty = true;
        self.check_dirty = true;
        self.transcode.clear();
        self.transcode_dirty = true;
    }

    fn mark_dirty(&mut self) {
        self.usage_dirty = true;
        self.check_dirty = true;
        self.transcode_dirty = true;
    }

    fn validate_selection(&mut self) {
        match self.selection {
            Selection::Asset(id) => {
                if !self.session.project.assets.contains_key(&id) {
                    self.selection = Selection::None;
                }
            }
            Selection::Timeline(id) => {
                if self.session.project.timeline(id).is_none() {
                    self.selection = Selection::None;
                }
            }
            Selection::Menu(id) => match self.session.project.menu(id) {
                Some(menu) => {
                    // 取り消しなどでボタンが減ったら、なくなった番号を選択から外す。
                    let count = menu.buttons.len();
                    self.selected_buttons.retain(|index| *index < count);
                }
                None => {
                    self.selection = Selection::None;
                    self.menu_texture = None;
                }
            },
            Selection::Slideshow(id) => {
                if !self.session.project.slideshows.contains_key(&id) {
                    self.selection = Selection::None;
                }
            }
            Selection::Playlist(id) => {
                if !self.session.project.playlists.contains_key(&id) {
                    self.selection = Selection::None;
                }
            }
            Selection::ChapterPlaylist(id) => {
                if !self.session.project.chapter_playlists.contains_key(&id) {
                    self.selection = Selection::None;
                }
            }
            Selection::None => {}
        }
    }

    /// タイムラインビューアでチャプターポイントを選んだ（`None` は選択を外した）。タイムラインを選択し、
    /// プロパティパネルにチャプターのシートを出す。
    pub(crate) fn focus_chapter(&mut self, timeline: TimelineId, index: Option<usize>) {
        match index {
            Some(index) => {
                self.select(Selection::Timeline(timeline));
                self.chapter_focus = Some((timeline, index));
            }
            None => {
                if self
                    .chapter_focus
                    .is_some_and(|(focused, _)| focused == timeline)
                {
                    self.chapter_focus = None;
                }
            }
        }
    }

    pub(crate) fn select(&mut self, selection: Selection) {
        self.select_showing(selection, true);
    }

    /// Bring the viewer of `selection` forward (Encore opens it on a double click).
    pub(crate) fn show_viewer(&mut self, selection: Selection) {
        let panel = match selection {
            Selection::Menu(_) => Panel::MenuViewer,
            Selection::Timeline(_) => Panel::TimelineViewer,
            Selection::Slideshow(_) => Panel::SlideshowViewer,
            Selection::Playlist(_) => Panel::Properties,
            Selection::ChapterPlaylist(_) => Panel::ChapterPlaylist,
            Selection::Asset(_) | Selection::None => return,
        };
        self.show_panel(panel);
    }

    /// [`Self::select`]. `show`: bring the item's viewer forward (the flowchart's click does not, as Encore's: its
    /// double-click opens the item).
    pub(crate) fn select_showing(&mut self, selection: Selection, show: bool) {
        // フローチャートでクリックした場所は、選び直したら忘れる（フローチャートのクリックは選んだ後で覚え直す）。
        self.flow_clicked = None;
        // 項目を選び直したら（同じタイムラインやメニューでも）プロパティはその項目のシートに戻す。
        self.chapter_focus = None;
        self.cue_focus = None;
        self.slide_focus = None;
        self.selected_buttons.clear();
        self.selected_text = None;
        self.selected_graphic = None;
        self.selected_part = None;
        if self.selection == selection {
            return;
        }

        self.selection = selection;

        match selection {
            Selection::Menu(id) => self.last_menu = Some(id),
            Selection::Timeline(id) => self.last_timeline = Some(id),
            Selection::Slideshow(id) => self.last_slideshow = Some(id),
            Selection::ChapterPlaylist(id) => self.last_chapter_playlist = Some(id),
            Selection::Playlist(_) | Selection::Asset(_) | Selection::None => {}
        }
        if show {
            self.show_viewer(selection);
        }

        self.selected_buttons.clear();
        self.button_label_menu = None;
        self.button_label_button = None;
        self.button_label.clear();
        self.color_menu = None;
        self.menu_texture = None;
        self.rename_key = None;
        self.rename_value.clear();
        self.canvas_drag = None;
        self.drag_rects.clear();
        self.marquee = None;
    }

    pub(crate) fn run_command(&mut self, id: &str, params: Value) -> Option<Value> {
        match self.session.execute(id, &params) {
            Ok(value) => {
                crate::help::log(id);
                // 字幕の編集と取り消し / やり直しのあとは、覚えている字幕を読み直す（編集した字幕はプロジェクトにある）。
                if id.starts_with("subtitle.") || id.starts_with("edit.") {
                    self.subtitle_cache.clear();
                }
                self.status = summarize_result(&value);
                self.mark_dirty();
                Some(value)
            }
            Err(error) => {
                crate::help::log(&format!("{id} failed: {error}"));
                self.status = error;
                None
            }
        }
    }

    /// 読み取り専用のクエリを実行する。ステータス欄と dirty フラグを変えない
    /// （モニターのように毎フレーム呼び出しうるコマンド用）。
    /// [`Self::run_query`] for what may fail as a matter of course (a thumbnail or a frame of a video whose
    /// picture does not decode, of a file that is offline): nothing is said in the status bar; the caller shows
    /// the lack of a picture where the picture goes.
    pub(crate) fn run_query_quiet(&mut self, id: &str, params: Value) -> Option<Value> {
        self.session.execute(id, &params).ok()
    }

    pub(crate) fn run_query(&mut self, id: &str, params: Value) -> Option<Value> {
        match self.session.execute(id, &params) {
            Ok(value) => Some(value),
            Err(error) => {
                self.status = error;
                None
            }
        }
    }

    /// Whether a media file of the project is still being opened (each is opened on a thread of its own:
    /// `asset.pending`). While one is, the status bar says so and the window is drawn again shortly; when the
    /// last is open, what was shown without it (lengths, thumbnails) is asked for again.
    fn wait_for_media(&mut self, ctx: &egui::Context) -> bool {
        let answer = self.session.execute("asset.pending", &json!({})).ok();
        let listed = |name: &str| {
            answer
                .as_ref()
                .and_then(|value| value.get(name))
                .and_then(Value::as_array)
                .is_some_and(|ids| !ids.is_empty())
        };
        let pending = listed("pending");
        // A file whose length is known but whose pictures are not open yet holds nothing back; the window is
        // drawn again now and then until its thumbnail and frames can be had.
        if !pending && listed("opening") {
            self.media_pending = true;
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            return false;
        }
        if pending {
            if !self.media_pending {
                self.status = tl!("Reading media…").to_string();
            }
            self.media_pending = true;
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        } else if self.media_pending {
            self.media_pending = false;
            // What the check says of a file can change once it is open (a picture that does not decode).
            self.check_dirty = true;
            if self.status == tl!("Reading media…") {
                self.status = tl!("Media read").to_string();
            }
            ctx.request_repaint();
        }
        pending
    }

    /// 編集のたびに自動で走る容量の見積もり。ステータスバーの表示は変えない（失敗したときはエラーを残す）。
    pub(crate) fn refresh_usage(&mut self) {
        self.usage_dirty = false;
        // A query: through `run_command` it would mark the project edited and start the check, which started
        // this again, on every frame.
        if let Some(value) = self.run_query("disc.usage", json!({})) {
            self.usage = parse_usage(&value);
        }
    }

    /// 利用者が頼んだ容量の更新。結果をステータスバーに出す。
    pub(crate) fn update_usage(&mut self) {
        self.refresh_usage();
        self.status = tl!("Usage updated").to_string();
    }

    /// 編集のたびに自動で走るチェック。ステータスバーの表示（保存しました など）は変えない。
    pub(crate) fn refresh_check(&mut self) {
        self.check_dirty = false;
        // A query, as `refresh_usage`. When it fails, the error stays in the status bar.
        if let Some(value) = self.run_query("project.check", json!({})) {
            self.problems = parse_problems(&value);
        }
    }

    /// 利用者が頼んだチェック（プロジェクトをチェック、更新ボタン）。結果をステータスバーに出す。
    pub(crate) fn check_project(&mut self) {
        self.refresh_check();
        self.status = tl!("Check updated").to_string();
    }

    fn handle_build_result(&mut self, result: Result<BuildReport, String>) {
        // The image of a disc to write (M9.35): written now, once for each copy.
        if self.disc.building {
            self.disc.building = false;
            match result {
                Ok(_) => {
                    self.disc.copy = 1;
                    self.refresh_media();
                    self.start_writing();
                }
                Err(error) => self.end_writing(if disccraft_build::was_stopped(&error) {
                    tl!("Build stopped").to_owned()
                } else {
                    error
                }),
            }
            return;
        }
        match result {
            Ok(report) => {
                self.status = tlf!("Build finished: {path}", path = report.output.display());
                // M6.515: a DVD image's layer break goes into the project (saved with it).
                if self.build_job.output_kind == Some(disccraft_build::OutputKind::Iso)
                    && self.session.project.settings.kind == disccraft_project::DiscKind::Dvd
                    && self.session.project.note_built_image(
                        &report.output,
                        report.total_bytes,
                        report.layer_break.clone(),
                    )
                {
                    self.session.dirty = true;
                }
                self.last_build = Some(report);
                self.last_output_kind = self.build_job.output_kind;
                self.simulate = None;
                self.transcode_dirty = true;
            }
            Err(error) => {
                if disccraft_build::was_stopped(&error) {
                    self.status = tl!("Build stopped").to_owned();
                } else {
                    // M6.503: said in a dialog, as Encore does. The panel only went back to its settings, with the
                    // error in the status bar and under Result, out of sight at the panel's foot.
                    self.status = error.clone();
                    self.build_failure = Some(error);
                }
                self.simulate = None;
            }
        }
    }

    pub(crate) fn import_assets(&mut self) {
        let Some(paths) = FileDialog::new()
            .add_filter(tl!("Media"), &crate::file_drop::MEDIA_EXTENSIONS)
            .pick_files()
        else {
            self.status = tl!("Import cancelled").to_string();
            return;
        };

        for path in paths {
            self.import_asset(&path);
        }
    }

    /// File ▸ Import ▸ Timeline… (Encore's タイムラインとして読み込み): each chosen video becomes an asset and a
    /// timeline named after it, in one undo step. The last one opens in the Timeline panel; the first becomes First
    /// Play when nothing plays first yet (as a new timeline does).
    pub(crate) fn import_timeline_files(&mut self) {
        let Some(paths) = FileDialog::new()
            .set_title(tl!("Import as Timeline"))
            .add_filter(
                tl!("Video files"),
                &[
                    "mp4", "mov", "m4v", "avi", "mkv", "mxf", "m2v", "m1v", "mpg", "mpeg", "vob",
                    "m2ts", "ts", "dv", "dif",
                ],
            )
            .pick_files()
        else {
            return;
        };
        let paths: Vec<Value> = paths
            .iter()
            .map(|path| Value::from(path.to_string_lossy().into_owned()))
            .collect();
        let Some(imported) = self.run_command("timeline.import", json!({ "paths": paths })) else {
            return;
        };
        let ids = |key: &str| -> Vec<u64> {
            imported
                .get(key)
                .and_then(Value::as_array)
                .map(|ids| ids.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default()
        };
        let (assets, timelines) = (ids("assets"), ids("timelines"));
        for id in assets.iter().chain(&timelines) {
            self.place_new_item(*id);
        }
        if let Some(first) = timelines.first()
            && self.session.project.settings.first_play == Link::NotSet
        {
            self.set_first_play(&Link::Timeline {
                id: TimelineId(*first),
                chapter: 0,
            });
        }
        if let Some(last) = timelines.last() {
            self.select(Selection::Timeline(TimelineId(*last)));
        }
        self.status = tl!("Imported as timelines").to_owned();
    }

    /// File ▸ Import ▸ Slideshow… (Encore's スライドショーとして読み込み): the chosen images become the slides of one
    /// new slideshow (in file name order, named after the first), which opens in the Slideshow panel.
    pub(crate) fn import_slideshow_files(&mut self) {
        let Some(paths) = FileDialog::new()
            .set_title(tl!("Import as Slideshow"))
            .add_filter(
                tl!("Image files"),
                &[
                    "png", "jpg", "jpeg", "bmp", "tif", "tiff", "psd", "pcraft", "svg",
                ],
            )
            .pick_files()
        else {
            return;
        };
        let paths: Vec<Value> = paths
            .iter()
            .map(|path| Value::from(path.to_string_lossy().into_owned()))
            .collect();
        let Some(imported) = self.run_command("slideshow.import", json!({ "paths": paths })) else {
            return;
        };
        let assets: Vec<u64> = imported
            .get("assets")
            .and_then(Value::as_array)
            .map(|ids| ids.iter().filter_map(Value::as_u64).collect())
            .unwrap_or_default();
        for id in &assets {
            self.place_new_item(*id);
        }
        if let Some(id) = imported.get("slideshow").and_then(Value::as_u64) {
            self.place_new_item(id);
            self.select(Selection::Slideshow(id));
        }
        self.status = tl!("Imported as a slideshow").to_owned();
    }

    fn import_asset(&mut self, path: &Path) {
        let _ = self.import_asset_id(path);
    }

    /// Imports `path` as an asset (placed in the Project panel's open folder); its id, or `None` when it could not
    /// be imported (the status bar says why).
    pub(crate) fn import_asset_id(&mut self, path: &Path) -> Option<u64> {
        let imported = self.run_command("asset.import", crate::gui_params::import_asset(path));
        let id = imported.and_then(|value| value.get("asset").and_then(Value::as_u64))?;
        self.place_new_item(id);
        Some(id)
    }

    /// Encore の「アセットを置き換え…」「アセットを検索…」: 別のファイルを選んで差し替える（名前とリンクはそのまま。
    /// 同じ種類の素材だけ）。覚えていた長さやサムネールは取り直す。
    pub(crate) fn relink_asset(&mut self, id: AssetId) {
        let Some(asset) = self.session.project.assets.get(&id).cloned() else {
            return;
        };
        let mut dialog = FileDialog::new().set_title(tl!("Locate Asset"));
        if let Some(parent) = Path::new(&asset.path).parent().filter(|dir| dir.is_dir()) {
            dialog = dialog.set_directory(parent);
        }
        let Some(path) = dialog.pick_file() else {
            return;
        };
        if self
            .run_command(
                "asset.setPath",
                serde_json::json!({ "asset": id.0, "path": path.to_string_lossy() }),
            )
            .is_some()
        {
            let _ = self.durations.remove(&id.0);
            let _ = self.thumbnails.remove(&id.0);
            self.menu_thumbnails.clear();
            self.monitor_frame = None;
            self.transcode_dirty = true;
        }
    }

    pub(crate) fn create_timeline(&mut self, asset_id: AssetId) {
        let Some(video) = self.asset_id_value(&asset_id) else {
            return;
        };

        let created =
            self.run_command("timeline.create", crate::gui_params::timeline_create(video));
        let id = created.and_then(|value| value.get("timeline").and_then(Value::as_u64));
        if let Some(id) = id {
            self.place_new_item(id);
        }

        // Like Encore: the first timeline becomes the disc's First Play.
        if let Some(id) = id
            && self.session.project.settings.first_play == Link::NotSet
        {
            self.set_first_play(&Link::Timeline {
                id: TimelineId(id),
                chapter: 0,
            });
        }
    }

    pub(crate) fn create_menu(&mut self) {
        let created = self.run_command("menu.create", json!({ "name": tl!("Menu") }));
        let id = created.and_then(|value| value.get("menu").and_then(Value::as_u64));

        // Like Encore: the first menu becomes the disc's Title button (the remote's Title / Top Menu key), and
        // First Play when nothing plays first yet.
        if let Some(id) = id
            && self.session.project.settings.title_remote == Link::NotSet
        {
            self.set_title_remote(&Link::Menu {
                id: MenuId(id),
                button: None,
            });
        }
        if let Some(id) = id
            && self.session.project.settings.first_play == Link::NotSet
        {
            self.set_first_play(&Link::Menu {
                id: MenuId(id),
                button: None,
            });
        }
        // The new menu opens in the menu viewer.
        if let Some(id) = id {
            self.place_new_item(id);
            self.select(Selection::Menu(MenuId(id)));
        }
    }

    /// 新規 ▸ で作った項目を、フォルダーの行から作ったならそのフォルダーへ入れる。
    pub(crate) fn place_new_item(&mut self, id: u64) {
        if let Some(folder) = self.new_item_folder {
            let _ = self.run_command(
                "project.moveToFolder",
                json!({ "id": id, "folder": folder }),
            );
        }
    }

    /// 新規フォルダー（`parent` の中、または root）。名前の入力を始める。
    pub(crate) fn create_folder(&mut self, parent: Option<u64>) {
        let mut params = serde_json::Map::new();
        let _ = params.insert("name".to_owned(), Value::from(tl!("Folder")));
        if let Some(parent) = parent {
            let _ = params.insert("parent".to_owned(), Value::from(parent));
        }
        let id = self
            .run_command("project.createFolder", Value::Object(params))
            .and_then(|value| value.get("folder").and_then(Value::as_u64));
        if let Some(id) = id {
            self.collapsed_folders
                .retain(|folder| Some(*folder) != parent);
            self.renaming_folder = Some((id, tl!("Folder").to_owned()));
        }
    }

    /// 新規 ▸ 再生リスト…: Encore と同じく、まず名前を聞く。
    pub(crate) fn create_playlist(&mut self) {
        self.playlist_name_dialog = Some(tl!("Untitled Playlist").to_string());
    }

    /// 名前の決まった再生リストを作り、選ぶ。
    pub(crate) fn create_playlist_named(&mut self, name: &str) {
        let name = name.trim();
        let name = if name.is_empty() {
            tl!("Untitled Playlist")
        } else {
            name
        };
        let id = self
            .run_command("playlist.create", crate::gui_params::create_named(name))
            .and_then(|value| value.get("playlist").and_then(Value::as_u64));

        if let Some(id) = id {
            self.place_new_item(id);
            self.select(Selection::Playlist(id));
        }
    }

    /// 新規チャプター再生リスト（Encore: タイムラインの右クリック 新規 ▸ チャプター再生リスト）。名前は
    /// 「<タイムライン> チャプター再生リスト」。作るとビューアに開く。
    pub(crate) fn create_chapter_playlist(&mut self, timeline: TimelineId) {
        let name = self
            .session
            .project
            .timeline(timeline)
            .map(|timeline| tlf!("{name} Chapter Playlist", name = timeline.name));
        let mut params = serde_json::json!({ "timeline": timeline.0 });
        if let (Some(name), Some(map)) = (name, params.as_object_mut()) {
            let _ = map.insert("name".to_owned(), Value::from(name));
        }
        let id = self
            .run_command("chapterplaylist.create", params)
            .and_then(|value| value.get("chapterPlaylist").and_then(Value::as_u64));

        if let Some(id) = id {
            self.place_new_item(id);
            self.select(Selection::ChapterPlaylist(id));
        }
    }

    pub(crate) fn create_slideshow(&mut self) {
        let id = self
            .run_command(
                "slideshow.create",
                crate::gui_params::create_slideshow(
                    tl!("Slideshows"),
                    self.timeline_prefs.still_seconds,
                ),
            )
            .and_then(|value| value.get("slideshow").and_then(Value::as_u64));

        if let Some(id) = id {
            self.place_new_item(id);
            self.selection = Selection::Slideshow(id);
        }
    }

    pub(crate) fn rename_selection(&mut self, name: &str) {
        let Some((kind, id)) = self.selection_key() else {
            self.status = tl!("Choose an item to rename").to_string();
            return;
        };

        self.run_command("project.rename", crate::gui_params::rename(kind, id, name));
    }

    /// 選択中の項目の `project.rename` 用の種別と ID。
    /// 項目を丸ごと削除する（`project.remove`）。リンクは未設定に戻り、取り消しで元に戻せる。
    pub(crate) fn remove_object(&mut self, selection: Selection) {
        let Some((kind, id)) = selection_key_of(selection) else {
            return;
        };
        // 使用中のアセットは消せない。先に分かりやすい言葉で知らせる。
        if let Selection::Asset(asset) = selection
            && let Some(user) = self.session.project.asset_user(asset)
        {
            self.status = tlf!("Cannot delete: {user} uses this asset.", user = user);
            return;
        }
        let removed = self.run_command("project.remove", json!({ "kind": kind, "id": id }));
        if removed.is_some() {
            if self.selection == selection {
                self.select(Selection::None);
            }
            self.status = tl!("Deleted").to_string();
        }
    }

    pub(crate) fn selection_key(&self) -> Option<(&'static str, u64)> {
        selection_key_of(self.selection)
    }

    /// スライドを足す（デュレーションはスライドショーの設定）。
    pub(crate) fn add_slide(&mut self, slideshow: u64, asset: AssetId) {
        let Some(asset) = self.asset_id_value(&asset) else {
            return;
        };

        self.run_command(
            "slideshow.addSlide",
            crate::gui_params::add_slide(slideshow, asset, None),
        );
    }

    pub(crate) fn remove_slide(&mut self, slideshow: u64, index: usize) {
        self.run_command(
            "slideshow.removeSlide",
            crate::gui_params::remove_slide(slideshow, index),
        );
    }

    /// 再生リストに項目（タイムライン / スライドショー / チャプター再生リスト）を足す。足した項目を選ぶ。
    pub(crate) fn add_playlist_entry(
        &mut self,
        playlist: u64,
        item: disccraft_project::PlaylistItem,
    ) {
        let added = self.run_command(
            "playlist.addItem",
            crate::gui_params::playlist_entry(playlist, item),
        );
        if let Some(index) = added
            .as_ref()
            .and_then(|value| value.get("item"))
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
        {
            self.playlist_item_pick = Some((playlist, index));
        }
    }

    pub(crate) fn remove_playlist_item(&mut self, playlist: u64, index: usize) {
        let removed = self.run_command(
            "playlist.removeItem",
            crate::gui_params::remove_playlist_item(playlist, index),
        );
        if removed.is_some() {
            let remaining = self
                .session
                .project
                .playlist(playlist)
                .map_or(0, |list| list.items.len());
            self.playlist_item_pick = (remaining > 0).then(|| (playlist, index.min(remaining - 1)));
        }
    }

    pub(crate) fn move_playlist_item(&mut self, playlist: u64, from: usize, to: usize) {
        let moved = self.run_command(
            "playlist.moveItem",
            json!({ "playlist": playlist, "from": from, "to": to }),
        );
        if moved.is_some() {
            self.playlist_item_pick = Some((playlist, to));
        }
    }

    pub(crate) fn set_slideshow_end_action(&mut self, slideshow: u64, link: &Link) {
        self.run_link_command(
            "slideshow.setEndAction",
            json!({ "slideshow": slideshow }),
            link,
        );
    }

    pub(crate) fn set_playlist_end_action(&mut self, playlist: u64, link: &Link) {
        self.run_link_command(
            "playlist.setEndAction",
            json!({ "playlist": playlist }),
            link,
        );
    }

    pub(crate) fn set_chapter_playlist_end_action(&mut self, playlist: u64, link: &Link) {
        self.run_link_command(
            "chapterplaylist.setEndAction",
            json!({ "chapterPlaylist": playlist }),
            link,
        );
    }

    pub(crate) fn set_default_button(&mut self, menu_id: MenuId, index: usize) {
        let Some(menu) = self.menu_id_value(&menu_id) else {
            return;
        };

        self.run_command(
            "menu.setDefaultButton",
            crate::gui_params::set_default_button(menu, index),
        );
    }

    /// ボタンの位置と大きさを変える（`button.setRect`）。
    /// 失敗時は `Session` が自動でロールバックするので、ドラッグ中の表示だけがずれる。
    pub(crate) fn set_button_rect(&mut self, menu_id: MenuId, index: usize, rect: Rect) {
        let Some(menu) = self.menu_id_value(&menu_id) else {
            return;
        };

        self.run_command(
            "button.setRect",
            crate::gui_params::set_button_rect(menu, index, rect),
        );
    }

    pub(crate) fn set_end_action(&mut self, timeline_id: TimelineId, link: &Link) {
        let Some(timeline) = self.timeline_id_value(&timeline_id) else {
            return;
        };

        self.run_link_command(
            "timeline.setEndAction",
            json!({ "timeline": timeline }),
            link,
        );
    }

    /// メニューのエンドアクション（静止メニューのデュレーションが過ぎたとき、モーションメニューが 1 回流れたとき）。
    pub(crate) fn set_menu_end_action(&mut self, menu_id: MenuId, link: &Link) {
        let Some(menu) = self.menu_id_value(&menu_id) else {
            return;
        };
        self.run_link_command("menu.setEndAction", json!({ "menu": menu }), link);
    }

    /// 静止メニューのデュレーション（秒）。`None` はボタンが押されるまで出したまま（静止）。
    pub(crate) fn set_menu_duration(&mut self, menu_id: MenuId, seconds: Option<u32>) {
        let Some(menu) = self.menu_id_value(&menu_id) else {
            return;
        };
        self.run_command(
            "menu.setDuration",
            crate::gui_params::menu_duration(menu, seconds),
        );
    }

    /// タイムラインのリモートメニュー（再生中にリモコンのメニューボタンを押したときの飛び先）。
    pub(crate) fn set_menu_remote(&mut self, timeline_id: TimelineId, link: &Link) {
        let Some(timeline) = self.timeline_id_value(&timeline_id) else {
            return;
        };

        self.run_link_command(
            "timeline.setMenuRemote",
            json!({ "timeline": timeline }),
            link,
        );
    }

    pub(crate) fn add_button(&mut self, menu_id: MenuId) {
        let Some(menu) = self.menu_id_value(&menu_id) else {
            return;
        };

        self.run_command(
            "menu.addButton",
            json!({ "menu": menu, "label": tl!("Button") }),
        );
    }

    pub(crate) fn set_button_label(&mut self, menu_id: MenuId, index: usize) {
        let Some(menu) = self.menu_id_value(&menu_id) else {
            return;
        };

        self.run_command(
            "button.setLabel",
            crate::gui_params::button_label(menu, index, &self.button_label),
        );
    }

    pub(crate) fn set_button_link(&mut self, menu_id: MenuId, index: usize, link: &Link) {
        let Some(menu) = self.menu_id_value(&menu_id) else {
            return;
        };

        let button = u64::try_from(index).unwrap_or(u64::MAX);

        self.run_link_command(
            "button.setLink",
            json!({ "menu": menu, "button": button }),
            link,
        );
    }

    pub(crate) fn set_background_color(&mut self, menu_id: MenuId) {
        let Some(menu) = self.menu_id_value(&menu_id) else {
            return;
        };

        self.run_command(
            "menu.setBackground",
            crate::gui_params::background_color(menu, rgb_from_f32(self.bg_color)),
        );
    }

    /// 動画の背景（モーションメニュー）。`seconds` はループの長さ。
    pub(crate) fn set_background_video(&mut self, menu_id: MenuId, asset: AssetId, seconds: u32) {
        self.run_command(
            "menu.setBackground",
            json!({ "menu": menu_id.0, "video": asset.0, "seconds": seconds }),
        );
    }

    pub(crate) fn set_background_image(&mut self, menu_id: MenuId, asset: AssetId) {
        let Some(menu) = self.menu_id_value(&menu_id) else {
            return;
        };

        let Some(asset) = self.asset_id_value(&asset) else {
            return;
        };

        self.run_command(
            "menu.setBackground",
            crate::gui_params::background_image(menu, asset),
        );
    }

    pub(crate) fn set_first_play(&mut self, link: &Link) {
        self.run_link_command("nav.setFirstPlay", json!({}), link);
    }

    pub(crate) fn set_title_remote(&mut self, link: &Link) {
        self.run_link_command("nav.setTitleRemote", json!({}), link);
    }

    pub(crate) fn set_disc_kind(&mut self, kind: DiscKind) {
        match serde_json::to_value(kind) {
            Ok(value) => {
                self.run_command("disc.setKind", json!({ "kind": value }));
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    pub(crate) fn set_disc_standard(&mut self, standard: VideoStandard) {
        match serde_json::to_value(standard) {
            Ok(value) => {
                self.run_command("disc.setStandard", json!({ "standard": value }));
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    pub(crate) fn set_capacity(&mut self, capacity: Capacity) {
        match serde_json::to_value(capacity) {
            Ok(value) => {
                self.run_command("disc.setCapacity", json!({ "capacity": value }));
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn asset_id_value(&mut self, id: &AssetId) -> Option<u64> {
        match asset_id_u64(id) {
            Ok(value) => Some(value),
            Err(error) => {
                self.status = error;
                None
            }
        }
    }

    fn timeline_id_value(&mut self, id: &TimelineId) -> Option<u64> {
        match timeline_id_u64(id) {
            Ok(value) => Some(value),
            Err(error) => {
                self.status = error;
                None
            }
        }
    }

    fn menu_id_value(&mut self, id: &MenuId) -> Option<u64> {
        match menu_id_u64(id) {
            Ok(value) => Some(value),
            Err(error) => {
                self.status = error;
                None
            }
        }
    }

    /// [`Self::run_link_command`] for other modules (a link field that has no setter of its own).
    pub(crate) fn run_link_command_pub(&mut self, id: &str, base: Value, link: &Link) -> bool {
        self.run_link_command(id, base, link)
    }

    fn run_link_command(&mut self, id: &str, base: Value, link: &Link) -> bool {
        let Value::Object(map) = base else {
            self.status = tl!("The command parameters are invalid").to_string();
            return false;
        };

        let params = match crate::gui_params::with_target(map, link) {
            Ok(params) => params,
            Err(error) => {
                self.status = error;
                return false;
            }
        };

        self.run_command(id, params).is_some()
    }
}

/// 選択の種類と ID（`project.rename` / `project.remove` の kind と id）。
fn selection_key_of(selection: Selection) -> Option<(&'static str, u64)> {
    match selection {
        Selection::Asset(id) => Some(("asset", id.0)),
        Selection::Timeline(id) => Some(("timeline", id.0)),
        Selection::Menu(id) => Some(("menu", id.0)),
        Selection::Slideshow(id) => Some(("slideshow", id)),
        Selection::Playlist(id) => Some(("playlist", id)),
        Selection::ChapterPlaylist(id) => Some(("chapterPlaylist", id)),
        Selection::None => None,
    }
}

fn summarize_result(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }

    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return message.to_string();
    }

    tl!("Command succeeded").to_string()
}

fn value_to_u64(value: Value) -> Result<u64, String> {
    if let Some(number) = value.as_u64() {
        return Ok(number);
    }

    if let Some(text) = value.as_str() {
        return text.parse::<u64>().map_err(|error| error.to_string());
    }

    if let Some(object) = value.as_object() {
        for key in ["id", "value", "raw"] {
            if let Some(inner) = object.get(key) {
                if let Some(number) = inner.as_u64() {
                    return Ok(number);
                }

                if let Some(text) = inner.as_str()
                    && let Ok(number) = text.parse::<u64>()
                {
                    return Ok(number);
                }
            }
        }
    }

    Err(tl!("Cannot convert the ID to a number").to_string())
}

fn asset_id_u64(id: &AssetId) -> Result<u64, String> {
    value_to_u64(serde_json::to_value(id).map_err(|error| error.to_string())?)
}

fn timeline_id_u64(id: &TimelineId) -> Result<u64, String> {
    value_to_u64(serde_json::to_value(id).map_err(|error| error.to_string())?)
}

fn menu_id_u64(id: &MenuId) -> Result<u64, String> {
    value_to_u64(serde_json::to_value(id).map_err(|error| error.to_string())?)
}

/// Converts an RGB byte tuple to a lowercase hex string.
pub(crate) fn rgb_to_hex(rgb: [u8; 3]) -> String {
    let [r, g, b] = rgb;
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Parses a `#rrggbb` or `rrggbb` string into RGB bytes.
pub(crate) fn hex_to_rgb(hex: &str) -> Option<[u8; 3]> {
    let text = hex.trim();
    let text = text.strip_prefix('#').unwrap_or(text);

    let mut chunks = text.as_bytes().chunks(2);
    let r = parse_hex_pair(chunks.next()?)?;
    let g = parse_hex_pair(chunks.next()?)?;
    let b = parse_hex_pair(chunks.next()?)?;

    if chunks.next().is_some() {
        return None;
    }

    Some([r, g, b])
}

fn parse_hex_pair(chunk: &[u8]) -> Option<u8> {
    if chunk.len() != 2 {
        return None;
    }

    let text = std::str::from_utf8(chunk).ok()?;
    u8::from_str_radix(text, 16).ok()
}

pub(crate) fn rgb_to_f32(rgb: [u8; 3]) -> [f32; 3] {
    let [r, g, b] = rgb;
    [u8_to_f32(r), u8_to_f32(g), u8_to_f32(b)]
}

pub(crate) fn rgb_from_f32(rgb: [f32; 3]) -> [u8; 3] {
    let [r, g, b] = rgb;
    [f32_to_u8(r), f32_to_u8(g), f32_to_u8(b)]
}

fn u8_to_f32(value: u8) -> f32 {
    f32::from(value) / 255.0
}

fn f32_to_u8(value: f32) -> u8 {
    let scaled = (value.clamp(0.0, 1.0) * 255.0).round();
    if !scaled.is_finite() {
        return 0;
    }

    u8::try_from(scaled as i32).unwrap_or(0)
}

/// 開いたプロジェクトでタイムラインビューアに読み込むタイムライン: ファーストプレイがタイムラインならそれ、
/// なければいちばん初めに作ったもの。タイムラインがなければ `None`。
pub(crate) fn preload_timeline(
    project: &disccraft_project::Project,
) -> Option<disccraft_project::TimelineId> {
    let first_play = match &project.settings.first_play {
        disccraft_project::Link::Timeline { id, .. } => Some(*id),
        _ => None,
    };
    first_play
        .filter(|id| project.timeline(*id).is_some())
        .or_else(|| project.timelines.keys().next().copied())
}

#[cfg(test)]
mod preload_tests {
    use super::preload_timeline;
    use disccraft_project::{Link, Project};
    use std::path::Path;

    #[test]
    fn the_first_play_timeline_is_preloaded_else_the_first()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        assert_eq!(preload_timeline(&project), None);
        let asset = project.add_asset(Path::new("movie.m2v"))?;
        let main = project.add_timeline(asset, Some("本編".to_owned()))?;
        let extra = project.add_timeline(asset, Some("おまけ".to_owned()))?;
        assert_eq!(preload_timeline(&project), Some(main));
        project.settings.first_play = Link::Timeline {
            id: extra,
            chapter: 0,
        };
        assert_eq!(preload_timeline(&project), Some(extra));
        Ok(())
    }
}

#[cfg(test)]
mod refresh_tests {
    use super::DiscCraftApp;

    /// The automatic estimate and check leave nothing to do behind them. They used to run through the editing
    /// path, which marked the project edited again, so each started the other (and the transcode status) on
    /// every frame drawn.
    #[test]
    fn the_automatic_estimate_and_check_do_not_start_each_other() {
        let mut app = DiscCraftApp::new(&egui::Context::default());
        app.usage_dirty = true;
        app.check_dirty = true;
        app.transcode_dirty = false;
        app.refresh_usage();
        app.refresh_check();
        assert!(!app.usage_dirty, "the check asked for the estimate again");
        assert!(!app.check_dirty, "the estimate asked for the check again");
        assert!(
            !app.transcode_dirty,
            "the transcode status was asked for again"
        );
    }

    /// M6.503: a build that fails says so in a dialog (the error kept until OK); one that was stopped does not.
    #[test]
    fn a_failed_build_opens_the_failure_dialog() {
        let mut app = DiscCraftApp::new(&egui::Context::default());
        let error = "mux error: invalid input: mux rate exceeded at 7302.428 s".to_owned();
        app.handle_build_result(Err(error.clone()));
        assert_eq!(app.build_failure.as_deref(), Some(error.as_str()));
        assert_eq!(app.status, error);

        let mut app = DiscCraftApp::new(&egui::Context::default());
        app.handle_build_result(Err("the transcode was stopped".to_owned()));
        assert_eq!(app.build_failure, None);
    }
}
