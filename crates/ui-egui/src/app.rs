use crate::build_panel::{
    BuildJob, ProblemView, SimulateSummary, Usage, parse_problems, parse_usage,
};
use crate::i18n::{Language, tl, tlf};
use crate::menu_editor::{CanvasDrag, HighlightPreview, MenuTexture};
use crate::menubar::ViewRequest;
use crate::monitor::PreviewState;
use crate::welcome::{Welcome, WelcomeChoice};
use crate::workspace::{
    BottomTab, CenterTab, Layout, RightTab, WorkspacePreset, load_language, load_preset,
    load_recent,
};

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
    pub(crate) status: String,
    pub(crate) language: Language,
    pub(crate) workspace: WorkspacePreset,
    /// 今のパネル構成（プリセットから始まり、ウィンドウメニューで個別に切り替える）。
    pub(crate) layout: Layout,
    pub(crate) tab: CenterTab,
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
    pub(crate) chapter_entry_pick: Option<(u64, usize)>,
    /// アセットの `asset.probe` の結果（キーはアセット ID）。
    pub(crate) durations: BTreeMap<u64, crate::project_panel::ProbeInfo>,
    /// アセットのサムネール（`asset.thumbnail` の結果。作れなかったものは `None`）。
    pub(crate) thumbnails: BTreeMap<u64, Option<egui::TextureHandle>>,
    /// 映像アセットの最後のフレーム（タイムラインのビデオトラックの末尾に描く）。
    pub(crate) last_frames: BTreeMap<u64, Option<egui::TextureHandle>>,
    /// 起動時のようこそ画面と新規プロジェクトダイアログ。
    pub(crate) welcome: Welcome,
    /// 最近使ったプロジェクト（新しい順）。
    pub(crate) recent: Vec<PathBuf>,
    /// 「プロジェクトをチェック」のウィンドウ。
    pub(crate) check_window: crate::check_window::CheckWindow,
    /// 未保存の変更の確認のあとに実行する操作。
    pub(crate) pending_action: Option<crate::confirm::PendingAction>,
    /// 確認済みで、ウィンドウを閉じてよい。
    pub(crate) allow_close: bool,
    /// 環境設定ダイアログ。
    pub(crate) prefs: crate::prefs::PrefsWindow,
    /// ライブラリパネルの状態。
    pub(crate) library: crate::library_panel::LibraryState,
    /// ユーザーがライブラリに足した項目（`library.json`）。
    pub(crate) user_library: crate::user_library::UserLibrary,
    /// 「一定間隔でチャプターポイントを追加」ダイアログ。
    pub(crate) chapter_interval: Option<crate::context_menus::ChapterInterval>,
    /// モニターに映しているタイムラインのフレーム（アセット ID、フレーム番号、テクスチャ）。
    pub(crate) monitor_frame: Option<(u64, i64, egui::TextureHandle)>,
    /// 読んだ字幕ファイルの字幕（アセットの id ごと）。
    pub(crate) subtitle_cache: BTreeMap<u64, std::sync::Arc<Vec<crate::subtitles::SubtitleCue>>>,
    /// タイムライン ▸ 字幕を書き出し で選んだトラックのアセット（メニューを閉じてから保存する）。
    pub(crate) subtitle_export: Option<AssetId>,
    /// パネルを並べたときのウィンドウの大きさ。変わったら割合どおりに並べ直す。
    pub(crate) layout_size: egui::Vec2,
    /// パネルの配置の世代（並べ直すたびに増やし、パネルの id に混ぜて大きさを初期値に戻す）。
    pub(crate) layout_generation: u32,
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
    /// ドラッグ中のボタンのルート（ルートを手で決めるとき）。
    pub(crate) route_drag: Option<crate::route_drag::RouteDrag>,
    /// 開いている「ユーザー操作」ダイアログ。
    pub(crate) user_ops_dialog: Option<crate::user_ops::UserOpsDialog>,
    /// 開いている「再生リストの名前」ダイアログ（Encore の 新規 ▸ 再生リスト…）の入力中の名前。
    pub(crate) playlist_name_dialog: Option<String>,
    /// 開いている「リンクの指定」ダイアログ（Ctrl+L）。
    pub(crate) link_dialog: Option<crate::link_dialog::LinkDialog>,
    /// The Drop Shadow dialog while it is open.
    pub(crate) shadow_dialog: Option<crate::shadow_dialog::ShadowDialog>,
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
    /// メニュービューアでピクセルをテレビ画面の縦横比に補正する（Encore の「テレビ画面に合わせてメニュー
    /// ピクセルを校正」。外すと 720x480 をそのままの比率で出す）。
    pub(crate) correct_pixel_aspect: bool,
    /// 表ごとの並べ替え（表の id → 列と向き）。
    pub(crate) table_sorts: std::collections::HashMap<&'static str, crate::theme::TableSort>,
    pub(crate) first_frames:
        std::collections::HashMap<u64, Option<(std::sync::Arc<Vec<u8>>, usize, usize)>>,
    /// Frames of videos at a time (asset, ticks, for a video thumbnail: whether the first bright frame from there),
    /// for a pop-up menu's background and the video thumbnails in the editor.
    pub(crate) timed_frames: std::collections::HashMap<
        (u64, i64, Option<bool>),
        Option<(std::sync::Arc<Vec<u8>>, usize, usize)>,
    >,
    /// モニターに出す字幕トラック（タイムラインごと）。Encore の字幕トラックの見出しの切り替えと同じく 0 本か 1 本で、
    /// 表示の状態なので保存も取り消しもしない。ないときは 1 本目。
    pub(crate) shown_subtitles: std::collections::HashMap<TimelineId, Option<usize>>,
    /// タイムラインの再生（タイムライン ▸ 再生、Space）。
    pub(crate) playback: Option<crate::playback::Playback>,
    pub(crate) menu_texture: Option<MenuTexture>,
    /// メニュービューアで選んでいるボタン（選んだ順。最後が主で、プロパティはそのシートを出す）。
    pub(crate) selected_buttons: Vec<usize>,
    /// 編集 ▸ コピー / 切り取りしたボタン。
    pub(crate) button_clipboard: Vec<crate::button_edit::ClipButton>,
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
    pub(crate) show_safe_areas: bool,
    pub(crate) show_routing: bool,
    /// 表示 ▸ ガイドを表示 / ガイドにスナップ / ガイドをロック。
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
    /// タイムラインビューアで選んだチャプターポイント。プロパティパネルはこのときチャプターのシートを出す。
    pub(crate) chapter_focus: Option<(TimelineId, usize)>,
    /// チャプターのシートの入力欄が表しているチャプター（タイムラインの id, 番号）。
    pub(crate) chapter_key: Option<(u64, usize)>,
    /// チャプターのシートの名前の入力欄。
    pub(crate) chapter_name_value: String,
    /// チャプターのシートのインの入力欄。
    pub(crate) chapter_in_value: String,
    /// チャプターのシートのポスターフレームの入力欄。
    pub(crate) chapter_poster_value: String,
    pub(crate) highlight_preview: HighlightPreview,
    pub(crate) preview: PreviewState,
    pub(crate) build_output: OutputKind,
    pub(crate) build_path: String,
    pub(crate) build_job: BuildJob,
    pub(crate) last_build: Option<BuildReport>,
    pub(crate) last_output_kind: Option<OutputKind>,
    pub(crate) simulate: Option<SimulateSummary>,
    pub(crate) usage: Option<Usage>,
    pub(crate) problems: Vec<ProblemView>,
    pub(crate) usage_dirty: bool,
    pub(crate) check_dirty: bool,
    /// アセットごとのトランスコードステータス（`asset.transcodeStatus`）。
    pub(crate) transcode: BTreeMap<u64, crate::project_panel::TranscodeState>,
    /// トランスコードステータスを問い合わせたときのビルドの出力先。変わったら問い合わせ直す。
    pub(crate) transcode_for: Option<(String, OutputKind)>,
    pub(crate) transcode_dirty: bool,
}

impl DiscCraftApp {
    /// Creates a new application state.
    pub fn new(ctx: &Context) -> Self {
        ctx.request_repaint();

        let workspace = load_preset();
        let layout = workspace.layout();
        let tab = layout.tab;
        let language = load_language();
        crate::i18n::set_current(language);

        Self {
            session: Session::new(),
            selection: Selection::None,
            status: tl!("DiscCraft started").to_string(),
            language,
            workspace,
            layout,
            tab,
            focus_rename: false,
            view_request: None,
            last_menu: None,
            last_timeline: None,
            last_slideshow: None,
            last_chapter_playlist: None,
            chapter_playlist_pick: None,
            chapter_entry_pick: None,
            durations: BTreeMap::new(),
            thumbnails: BTreeMap::new(),
            last_frames: BTreeMap::new(),
            welcome: Welcome::opened(),
            recent: load_recent(),
            check_window: crate::check_window::CheckWindow::default(),
            pending_action: None,
            allow_close: false,
            prefs: crate::prefs::PrefsWindow::default(),
            library: crate::library_panel::LibraryState::default(),
            user_library: crate::user_library::UserLibrary::load(),
            chapter_interval: None,
            monitor_frame: None,
            playback: None,
            layout_size: egui::Vec2::ZERO,
            layout_generation: 0,
            monitor_zoom: crate::monitor::MonitorZoom::Fit,
            menu_zoom: crate::monitor::MonitorZoom::Fit,
            tool: crate::menu_text::Tool::Selection,
            selected_text: None,
            text_drag: None,
            selected_graphic: None,
            graphic_drag: None,
            route_drag: None,
            user_ops_dialog: None,
            playlist_name_dialog: None,
            link_dialog: None,
            shadow_dialog: None,
            color_set_dialog: None,
            menu_thumbnails: std::collections::HashMap::new(),
            first_frames: std::collections::HashMap::new(),
            timed_frames: std::collections::HashMap::new(),
            shown_subtitles: std::collections::HashMap::new(),
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
            correct_pixel_aspect: true,
            hidden_project_columns: crate::project_panel::ProjectColumn::ALL
                .iter()
                .enumerate()
                .filter(|(_, column)| !column.shown_by_default())
                .map(|(index, _)| index)
                .collect(),
            subtitle_cache: BTreeMap::new(),
            subtitle_export: None,
            menu_texture: None,
            selected_buttons: Vec::new(),
            button_clipboard: Vec::new(),
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
            show_safe_areas: true,
            show_routing: false,
            show_guides: true,
            snap_to_guides: true,
            lock_guides: false,
            guide_dialog: None,
            guide_drag: None,
            show_chapter_names: false,
            track_dialog: None,
            about_open: false,
            chapter_focus: None,
            chapter_key: None,
            chapter_name_value: String::new(),
            chapter_in_value: String::new(),
            chapter_poster_value: String::new(),
            highlight_preview: HighlightPreview::default(),
            preview: PreviewState::default(),
            build_output: OutputKind::Folder,
            build_path: String::new(),
            build_job: BuildJob::default(),
            last_build: None,
            last_output_kind: None,
            simulate: None,
            usage: None,
            problems: Vec::new(),
            usage_dirty: true,
            transcode: BTreeMap::new(),
            transcode_for: None,
            transcode_dirty: true,
            check_dirty: true,
        }
    }

    /// Opens a project file.
    pub fn open_path(&mut self, path: &Path) -> Result<(), String> {
        let session = Session::open(path)?;
        self.session = session;
        self.reset_state();
        self.welcome = Welcome::default();
        self.status = tlf!("Opened: {path}", path = path.display());
        self.remember(path);
        Ok(())
    }

    /// Draws the whole UI.
    pub fn ui(&mut self, ui: &mut Ui) {
        // Strings are looked up per thread; set the language before anything is drawn.
        crate::i18n::set_current(self.language);

        if let Some(result) = self.build_job.poll() {
            self.handle_build_result(result);
        }

        if self.build_job.is_running() {
            ui.ctx().request_repaint();
        }

        self.validate_selection();

        if self.usage_dirty {
            self.refresh_usage();
        }

        if self.check_dirty {
            self.refresh_check();
        }

        let target = (self.build_path.trim().to_owned(), self.build_output);
        if self.transcode_dirty || self.transcode_for.as_ref() != Some(&target) {
            self.refresh_transcode();
        }

        self.menu_bar(ui);
        self.poll_playback(ui.ctx());
        self.poll_external_edits(ui.ctx());
        self.draw_groups(ui);
        self.check_window(ui.ctx());
        self.prefs_window(ui.ctx());
        self.chapter_interval_dialog(ui.ctx());
        self.track_dialog_window(ui.ctx());
        self.guide_dialog_window(ui.ctx());
        self.user_ops_window(ui.ctx());
        self.playlist_name_window(ui.ctx());
        self.link_window(ui.ctx());
        self.shadow_window(ui.ctx());
        self.color_set_window(ui.ctx());
        self.about_window(ui.ctx());
        self.show_welcome(ui);
        self.show_confirm(ui.ctx());
        self.intercept_close(ui.ctx());
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
        }
    }

    /// 新規プロジェクトダイアログの結果でプロジェクトを作り直す。
    pub(crate) fn apply_new_project(&mut self, form: &crate::new_project::NewProjectForm) {
        self.new_project();
        self.set_disc_kind(form.kind);
        self.set_disc_standard(form.standard);
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

    /// ワークスペースプリセットを切り替えて設定に保存する。
    pub(crate) fn set_workspace(&mut self, preset: WorkspacePreset) {
        self.workspace = preset;
        self.layout = preset.layout();
        // パネルの大きさも、そのワークスペースの割合に戻す。
        self.layout_generation = self.layout_generation.wrapping_add(1);
        self.tab = self.layout.tab;
        self.status = tlf!("Workspace: {name}", name = preset.label());

        if let Err(error) = crate::workspace::save_preset(preset) {
            self.status = tlf!(
                "Workspace: {name} (save failed: {error})",
                name = preset.label(),
                error = error
            );
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
                self.status = tl!("Redone").to_string();
                self.mark_dirty();
            }
            Err(error) => self.status = error,
        }
    }

    fn reset_state(&mut self) {
        self.selection = Selection::None;
        self.last_menu = None;
        self.last_timeline = None;
        self.last_slideshow = None;
        self.last_chapter_playlist = None;
        self.chapter_playlist_pick = None;
        self.chapter_entry_pick = None;
        self.durations.clear();
        self.thumbnails.clear();
        self.last_frames.clear();
        self.monitor_frame = None;
        self.playback = None;
        self.subtitle_cache.clear();
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
        // 項目を選び直したら（同じタイムラインやメニューでも）プロパティはその項目のシートに戻す。
        self.chapter_focus = None;
        self.slide_focus = None;
        self.selected_buttons.clear();
        self.selected_text = None;
        self.selected_graphic = None;
        if self.selection == selection {
            return;
        }

        self.selection = selection;

        // 選択物に合わせてビューアのタブを出す（Encore はダブルクリックでビューアが開く）。
        match selection {
            Selection::Menu(id) => {
                self.tab = CenterTab::Menu;
                self.last_menu = Some(id);
            }
            Selection::Timeline(id) => {
                self.layout.bottom = true;
                self.layout.bottom_tab = BottomTab::Timeline;
                self.last_timeline = Some(id);
            }
            Selection::Slideshow(id) => {
                self.layout.bottom = true;
                self.layout.bottom_tab = BottomTab::Slideshow;
                self.last_slideshow = Some(id);
            }
            Selection::Playlist(_) => {
                self.layout.right = true;
                self.layout.right_tab = RightTab::Properties;
            }
            Selection::ChapterPlaylist(id) => {
                self.tab = CenterTab::ChapterPlaylist;
                self.last_chapter_playlist = Some(id);
            }
            Selection::Asset(_) | Selection::None => {}
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
                self.status = summarize_result(&value);
                self.mark_dirty();
                Some(value)
            }
            Err(error) => {
                self.status = error;
                None
            }
        }
    }

    /// 読み取り専用のクエリを実行する。ステータス欄と dirty フラグを変えない
    /// （モニターのように毎フレーム呼び出しうるコマンド用）。
    pub(crate) fn run_query(&mut self, id: &str, params: Value) -> Option<Value> {
        match self.session.execute(id, &params) {
            Ok(value) => Some(value),
            Err(error) => {
                self.status = error;
                None
            }
        }
    }

    /// 編集のたびに自動で走る容量の見積もり。ステータスバーの表示は変えない（失敗したときはエラーを残す）。
    pub(crate) fn refresh_usage(&mut self) {
        let status = std::mem::take(&mut self.status);
        self.usage_dirty = false;
        let value = self.run_command("disc.usage", json!({}));
        self.usage_dirty = false;

        if let Some(value) = value {
            self.status = status;
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
        let status = std::mem::take(&mut self.status);
        self.check_dirty = false;
        let value = self.run_command("project.check", json!({}));
        self.check_dirty = false;

        // 失敗したときは run_command が出したエラーを残す。
        if let Some(value) = value {
            self.status = status;
            self.problems = parse_problems(&value);
        }
    }

    /// 利用者が頼んだチェック（プロジェクトをチェック、更新ボタン）。結果をステータスバーに出す。
    pub(crate) fn check_project(&mut self) {
        self.refresh_check();
        self.status = tl!("Check updated").to_string();
    }

    fn handle_build_result(&mut self, result: Result<BuildReport, String>) {
        match result {
            Ok(report) => {
                self.status = tlf!("Build finished: {path}", path = report.output.display());
                self.last_build = Some(report);
                self.last_output_kind = self.build_job.output_kind;
                self.simulate = None;
                self.transcode_dirty = true;
            }
            Err(error) => {
                self.status = error;
                self.simulate = None;
            }
        }
    }

    pub(crate) fn import_assets(&mut self) {
        let Some(paths) = FileDialog::new()
            .add_filter(
                tl!("Media"),
                &[
                    "mp4", "mov", "m4v", "avi", "mkv", "m2v", "wav", "ac3", "mp3", "aac", "flac",
                    "png", "jpg", "jpeg", "bmp", "tif", "tiff", "psd", "svg", "srt",
                ],
            )
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
                    "mp4", "mov", "m4v", "avi", "mkv", "mxf", "m2v", "mpg", "mpeg", "vob", "m2ts",
                    "ts",
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
                &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "psd", "svg"],
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
        let imported = self.run_command("asset.import", crate::gui_params::import_asset(path));
        if let Some(id) = imported.and_then(|value| value.get("asset").and_then(Value::as_u64)) {
            self.place_new_item(id);
        }
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
                crate::gui_params::create_named(tl!("Slideshows")),
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
#[cfg(test)]
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

#[cfg(test)]
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
