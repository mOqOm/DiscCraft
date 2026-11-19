//! 環境設定ダイアログ（Encore と同じ形。編集 ▸ 環境設定、Ctrl+K。[docs/ui-parity.md](../../../docs/ui-parity.md) §3.5）。
//!
//! 左に分類の一覧、右にその設定を置く。「一般」はテレビ方式の初期設定（新規プロジェクトの既定）、
//! 「表示方法」は表示言語。設定は `workspace.json` に保存する。

use crate::DiscCraftApp;
use crate::i18n::{Language, tl, tlf};

use disccraft_project::VideoStandard;
use egui::{Context, Id, Ui};

/// 環境設定の分類。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum PrefsCategory {
    /// 一般（テレビ方式の初期設定）。
    #[default]
    General,
    /// Audio/Video Output (the audio output device).
    AudioVideo,
    /// タイムライン（静止画と字幕の長さ、トラックの言語）。
    Timelines,
    /// Preview (the player the previews stand for: TV mode, region, audio language, parental level).
    Preview,
    /// Media (where the library and the media cache are kept; emptying the cache).
    Media,
    /// メニュー（ボタンのルートとボタン同期の初期設定。プロジェクトに保存）。
    Menus,
    /// Memory (the installed RAM, and how many transcodes run at a time).
    Memory,
    /// 表示方法（表示言語）。
    Interface,
}

impl PrefsCategory {
    /// 左の一覧に並べる順。
    pub(crate) const ALL: [PrefsCategory; 8] = [
        PrefsCategory::General,
        PrefsCategory::AudioVideo,
        PrefsCategory::Timelines,
        PrefsCategory::Preview,
        PrefsCategory::Media,
        PrefsCategory::Menus,
        PrefsCategory::Memory,
        PrefsCategory::Interface,
    ];

    /// 表示名。
    pub(crate) fn label(self) -> &'static str {
        match self {
            PrefsCategory::General => tl!("General"),
            PrefsCategory::AudioVideo => tl!("Audio/Video Output"),
            PrefsCategory::Timelines => tl!("Timelines"),
            PrefsCategory::Preview => tl!("Preview"),
            PrefsCategory::Media => tl!("Media"),
            PrefsCategory::Menus => tl!("Menus"),
            PrefsCategory::Memory => tl!("Memory"),
            PrefsCategory::Interface => tl!("Interface"),
        }
    }
}

/// The height of the dialog's pages.
const PAGE_HEIGHT: f32 = 240.0;

/// 環境設定ダイアログの状態。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PrefsWindow {
    /// 表示中かどうか。
    pub(crate) open: bool,
    /// 選択中の分類。
    pub(crate) category: PrefsCategory,
}

impl DiscCraftApp {
    /// 環境設定ダイアログを描く（開いているときだけ）。
    pub(crate) fn prefs_window(&mut self, ctx: &Context) {
        if !self.prefs.open {
            return;
        }

        // As FilmCraft's Preferences: a dialog in the middle of the window, over the dimmed interface, with its
        // title on top. The settings take effect as they are changed, so the one button closes it (Esc and a click
        // outside do too).
        let mut close = false;
        let response = egui::Modal::new(Id::unique("prefs_window")).show(ctx, |ui| {
            // Wide enough for the widest page, so the dialog does not move as pages are chosen.
            ui.set_width(660.0);
            ui.vertical_centered(|ui| ui.strong(tl!("Preferences")));
            ui.separator();
            ui.add_space(4.0);
            self.prefs_contents(ui);
            ui.add_space(4.0);
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tl!("Close")).clicked() {
                    close = true;
                }
            });
        });
        if close || response.should_close() {
            self.prefs.open = false;
        }
    }

    /// 左の分類と右の設定。
    fn prefs_contents(&mut self, ui: &mut Ui) {
        ui.horizontal_top(|ui| {
            // As tall as its pages: the line between the list and the page would take all the height there is.
            ui.set_max_height(PAGE_HEIGHT);
            ui.vertical(|ui| {
                ui.set_width(150.0);
                for category in PrefsCategory::ALL {
                    if ui
                        .selectable_label(self.prefs.category == category, category.label())
                        .clicked()
                    {
                        self.prefs.category = category;
                    }
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_min_height(PAGE_HEIGHT);
                ui.set_width(ui.available_width());
                match self.prefs.category {
                    PrefsCategory::General => self.prefs_general(ui),
                    PrefsCategory::Timelines => self.prefs_timelines(ui),
                    PrefsCategory::Preview => self.prefs_preview(ui),
                    PrefsCategory::AudioVideo => self.prefs_audio_video(ui),
                    PrefsCategory::Media => self.prefs_media(ui),
                    PrefsCategory::Memory => self.prefs_memory(ui),
                    PrefsCategory::Menus => self.prefs_menus(ui),
                    PrefsCategory::Interface => self.prefs_interface(ui),
                }
            });
        });
    }

    /// 「一般」: テレビ方式の初期設定（新規プロジェクトの既定）。
    fn prefs_general(&mut self, ui: &mut Ui) {
        ui.strong(tl!("General"));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(tl!("Default TV standard:"));
            let current = crate::workspace::load_default_standard();
            let mut chosen = None;
            egui::ComboBox::from_id_salt("prefs_standard")
                .selected_text(standard_label(current))
                .show_ui(ui, |ui| {
                    for standard in [VideoStandard::Ntsc, VideoStandard::Pal] {
                        if ui
                            .selectable_label(current == standard, standard_label(standard))
                            .clicked()
                        {
                            chosen = Some(standard);
                        }
                    }
                });
            if let Some(standard) = chosen
                && let Err(error) = crate::workspace::save_default_standard(standard)
            {
                self.status = error;
            }
        });
        ui.small(tl!("Used as the TV standard of new projects."));
        ui.horizontal(|ui| {
            ui.label(tl!("Playback quality:"));
            let mut quality = self.playback_quality;
            egui::ComboBox::from_id_salt("prefs_playback_quality")
                .selected_text(quality.label())
                .show_ui(ui, |ui| {
                    for choice in crate::workspace::PlaybackQuality::ALL {
                        let _ = ui.selectable_value(&mut quality, choice, choice.label());
                    }
                });
            if quality != self.playback_quality {
                self.playback_quality = quality;
                if let Err(error) = crate::workspace::save_playback_quality(quality) {
                    self.status = error;
                }
            }
        });
        ui.small(tl!(
            "How large the Monitor plays a title's pictures: High at full size, Draft at half, Automatic at half only for HD larger than 1280 × 720."
        ));
    }

    /// 「タイムライン」（Encore: 静止画デュレーション / 字幕デュレーション / オーディオの言語 / 字幕の言語）。
    fn prefs_timelines(&mut self, ui: &mut Ui) {
        ui.strong(tl!("Timelines"));
        ui.add_space(6.0);
        let mut prefs = self.timeline_prefs.clone();
        ui.horizontal(|ui| {
            ui.label(tl!("Still duration:"));
            let _ = ui.add(
                egui::DragValue::new(&mut prefs.still_seconds)
                    .range(1..=3600)
                    .suffix(tl!(" s")),
            );
        });
        ui.small(tl!("The slide duration of new slideshows."));
        ui.horizontal(|ui| {
            ui.label(tl!("Subtitle duration:"));
            let _ = ui.add(
                egui::DragValue::new(&mut prefs.subtitle_seconds)
                    .range(1..=60)
                    .suffix(tl!(" s")),
            );
        });
        ui.small(tl!("The length of a subtitle added in the Monitor."));
        ui.horizontal(|ui| {
            ui.label(tl!("Audio language:"));
            let _ = ui.add(
                egui::TextEdit::singleline(&mut prefs.audio_language)
                    .char_limit(3)
                    .desired_width(40.0),
            );
        });
        ui.horizontal(|ui| {
            ui.label(tl!("Subtitle language:"));
            let _ = ui.add(
                egui::TextEdit::singleline(&mut prefs.subtitle_language)
                    .char_limit(3)
                    .desired_width(40.0),
            );
        });
        ui.small(tl!(
            "The language of new tracks (a code such as ja or en). Empty uses the disc's audio language."
        ));
        let _ = ui.checkbox(&mut prefs.snap_subtitles, tl!("Snap subtitle clips"));
        ui.small(tl!(
            "A subtitle clip dragged in a timeline snaps to the edges of the subtitle clips, the chapter points and the playhead near it."
        ));
        if prefs != self.timeline_prefs {
            // What is typed is kept as typed; the saved copy is tidied (a single letter is no language yet).
            if let Err(error) = crate::workspace::save_timeline_prefs(&prefs) {
                self.status = error;
            }
            self.timeline_prefs = prefs;
        }
    }

    /// "Audio/Video Output" (Encore: the audio output device, and video out to DV or another monitor): the device
    /// playback sounds on. Video goes to DiscCraft's own Monitor only.
    fn prefs_audio_video(&mut self, ui: &mut Ui) {
        ui.strong(tl!("Audio/Video Output"));
        ui.add_space(6.0);
        let current = crate::workspace::load_audio_device();
        let names = crate::audio_out::device_names();
        let mut chosen: Option<Option<String>> = None;
        ui.horizontal(|ui| {
            ui.label(tl!("Audio output device:"));
            egui::ComboBox::from_id_salt("prefs_audio_device")
                .width(300.0)
                .selected_text(
                    current
                        .clone()
                        .unwrap_or_else(|| tl!("System default").to_string()),
                )
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(current.is_none(), tl!("System default"))
                        .clicked()
                    {
                        chosen = Some(None);
                    }
                    for name in &names {
                        if ui
                            .selectable_label(current.as_deref() == Some(name.as_str()), name)
                            .clicked()
                        {
                            chosen = Some(Some(name.clone()));
                        }
                    }
                });
        });
        ui.small(tl!(
            "Where playback in the Monitor and the previews sounds, from the next time it starts. A device that is no longer there gives way to the system's default."
        ));
        if let Some(name) = chosen {
            match crate::workspace::save_audio_device(name.as_deref()) {
                Ok(()) => crate::audio_out::choose_device(name),
                Err(error) => self.status = error,
            }
        }

        // Video output (Encore: Video Devices, a tick for each connected monitor).
        ui.add_space(6.0);
        use crate::video_out::VideoOutput;
        let displays = disccraft_platform::displays();
        let label = |output: VideoOutput| match output {
            VideoOutput::Off => tl!("Off").to_string(),
            VideoOutput::Window => tl!("A window of its own").to_string(),
            VideoOutput::Display(number) => {
                let found = usize::try_from(number)
                    .ok()
                    .and_then(|number| number.checked_sub(1))
                    .and_then(|index| displays.get(index));
                match found {
                    Some(display) => tlf!(
                        "Display {n}: {w} x {h}",
                        n = number,
                        w = display.width,
                        h = display.height
                    ),
                    None => tlf!("Display {n} (not connected)", n = number),
                }
            }
        };
        let current = self.video_output;
        let mut output = current;
        ui.horizontal(|ui| {
            ui.label(tl!("Video output:"));
            egui::ComboBox::from_id_salt("prefs_video_output")
                .width(300.0)
                .selected_text(label(current))
                .show_ui(ui, |ui| {
                    let _ =
                        ui.selectable_value(&mut output, VideoOutput::Off, label(VideoOutput::Off));
                    let _ = ui.selectable_value(
                        &mut output,
                        VideoOutput::Window,
                        label(VideoOutput::Window),
                    );
                    for number in 1..=u32::try_from(displays.len()).unwrap_or(0) {
                        let choice = VideoOutput::Display(number);
                        let _ = ui.selectable_value(&mut output, choice, label(choice));
                    }
                });
        });
        let mut background_off = !self.video_output_in_background;
        let toggled = ui
            .checkbox(
                &mut background_off,
                tl!("Disable video output when in the background"),
            )
            .changed();
        ui.small(tl!(
            "The picture of the title the Monitor or a preview plays, shown full screen on another display or in a window of its own. Esc in that window turns it off."
        ));
        if toggled {
            self.video_output_in_background = !background_off;
        }
        if output != current || toggled {
            self.set_video_output(output);
        }
    }

    /// "Media" (Encore: Library Content / Media Cache Files / Media Cache Database with Clean): where DiscCraft
    /// keeps its library and what it remembers of media files (each can be put elsewhere), and emptying that.
    fn prefs_media(&mut self, ui: &mut Ui) {
        ui.strong(tl!("Media"));
        ui.add_space(6.0);
        // One place: where it is, Browse… to choose another, Default to go back, Show in Explorer. Returns the
        // change asked for: `Some(None)` is back to the default.
        let folder_row = |app: &mut Self,
                          ui: &mut Ui,
                          folder: Option<std::path::PathBuf>,
                          is_default: bool|
         -> Option<Option<std::path::PathBuf>> {
            let mut change = None;
            ui.horizontal(|ui| {
                ui.label(tl!("Location:"));
                match &folder {
                    Some(folder) => {
                        ui.add(
                            egui::Label::new(folder.display().to_string())
                                .truncate()
                                .selectable(true),
                        );
                    }
                    None => {
                        ui.weak(tl!("Unknown"));
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button(tl!("Browse…")).clicked() {
                    let mut dialog = rfd::FileDialog::new();
                    if let Some(folder) = folder.as_ref().filter(|folder| folder.is_dir()) {
                        dialog = dialog.set_directory(folder);
                    }
                    if let Some(chosen) = dialog.pick_folder() {
                        change = Some(Some(chosen));
                    }
                }
                if ui
                    .add_enabled(!is_default, egui::Button::new(tl!("Default")))
                    .clicked()
                {
                    change = Some(None);
                }
                if ui
                    .add_enabled(
                        folder.as_ref().is_some_and(|folder| folder.is_dir()),
                        egui::Button::new(tl!("Show in Explorer")),
                    )
                    .clicked()
                    && let Some(folder) = &folder
                {
                    app.reveal_in_file_manager(folder);
                }
            });
            change
        };

        ui.label(tl!("Library Content"));
        let library = crate::workspace::library_dir();
        let default = crate::workspace::default_library_dir();
        if let Some(chosen) = folder_row(self, ui, library.clone(), library == default) {
            self.move_library(chosen.as_deref());
        }
        ui.add_space(4.0);
        ui.label(tl!("Media Cache Database"));
        let cache = crate::workspace::media_cache_dir();
        let default = crate::workspace::default_media_cache_dir();
        if let Some(chosen) = folder_row(self, ui, cache.clone(), cache == default) {
            match crate::workspace::save_media_cache_dir(chosen.as_deref()) {
                Ok(()) => {
                    // From now on what is learnt of media files is kept there (what the folder already holds
                    // is used; the old folder is left as it is).
                    if let Some(dir) = crate::workspace::media_cache_dir() {
                        disccraft_engine::enable_media_cache(&dir);
                    }
                }
                Err(error) => self.status = error,
            }
        }
        if ui
            .add_enabled(cache.is_some(), egui::Button::new(tl!("Clean")))
            .on_hover_text(tl!(
                "Forgets the lengths, sizes and thumbnails remembered of media files; they are read again when needed"
            ))
            .clicked()
        {
            let removed = disccraft_engine::clear_media_cache();
            self.durations.clear();
            self.thumbnails.clear();
            self.status = tlf!("Media cache cleaned: {n} files", n = removed);
        }
    }

    /// Puts the library content in `folder` (`None`: back beside the settings). A folder that has a library
    /// already is opened as it is; an empty one gets the items the library has now.
    fn move_library(&mut self, folder: Option<&std::path::Path>) {
        if let Err(error) = crate::workspace::save_library_dir(folder) {
            self.status = error;
            return;
        }
        let has_library =
            crate::workspace::library_dir().is_some_and(|dir| dir.join("library.json").is_file());
        if has_library {
            self.user_library = crate::user_library::UserLibrary::load();
        } else if let Err(error) = self.user_library.save() {
            self.status = error;
        }
    }

    /// "Memory" (Encore: Installed RAM / RAM reserved for other applications): the installed RAM, and how many
    /// transcodes run at a time (each takes memory of its own, so fewer at a time leaves more for other
    /// applications).
    fn prefs_memory(&mut self, ui: &mut Ui) {
        ui.strong(tl!("Memory"));
        ui.add_space(6.0);
        let installed = disccraft_platform::installed_memory_bytes().map_or_else(
            || tl!("Unknown").to_string(),
            // `as`: a size shown to one decimal.
            |bytes| format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64),
        );
        ui.horizontal(|ui| {
            ui.label(tl!("Installed RAM:"));
            ui.label(installed);
        });
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        let cores = u32::try_from(cores).unwrap_or(1);
        let current = crate::workspace::load_parallel_transcodes().min(cores);
        let label = |most: u32| match most {
            0 => tlf!("Automatic ({n})", n = cores),
            most => most.to_string(),
        };
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.label(tl!("Transcodes at a time:"));
            egui::ComboBox::from_id_salt("prefs_parallel_transcodes")
                .selected_text(label(current))
                .show_ui(ui, |ui| {
                    for most in 0..=cores {
                        if ui.selectable_label(current == most, label(most)).clicked() {
                            chosen = Some(most);
                        }
                    }
                });
        });
        ui.small(tl!(
            "A build and Transcode Now convert several titles at once, one for every processor core. Each takes memory of its own: choose fewer to leave more for other applications."
        ));
        if let Some(most) = chosen {
            match crate::workspace::save_parallel_transcodes(most) {
                Ok(()) => {
                    disccraft_build::set_parallel_transcodes(usize::try_from(most).unwrap_or(0));
                }
                Err(error) => self.status = error,
            }
        }
    }

    /// "Preview" (Encore: TV Mode / Player Region Code / Audio Language; the parental level is DiscCraft's): the
    /// player the Project Preview and the Monitor stand for.
    fn prefs_preview(&mut self, ui: &mut Ui) {
        ui.strong(tl!("Preview"));
        ui.add_space(6.0);
        let mut prefs = self.preview_prefs.clone();
        ui.horizontal(|ui| {
            ui.label(tl!("TV mode:"));
            // (4:3 television, pan and scan).
            let label = |mode: (bool, bool)| match mode {
                (false, _) => tl!("16:9 Widescreen"),
                (true, false) => tl!("4:3 Letterbox"),
                (true, true) => tl!("4:3 Pan & Scan"),
            };
            let mut mode = (prefs.tv_4x3, prefs.pan_scan);
            egui::ComboBox::from_id_salt("prefs_tv_mode")
                .selected_text(label(mode))
                .show_ui(ui, |ui| {
                    for choice in [(false, false), (true, false), (true, true)] {
                        let _ = ui.selectable_value(&mut mode, choice, label(choice));
                    }
                });
            (prefs.tv_4x3, prefs.pan_scan) = mode;
        });
        ui.horizontal(|ui| {
            ui.label(tl!("Player region code:"));
            let label = |region: u8| match region {
                0 => tl!("All regions"),
                region => crate::build_view::region_label(region),
            };
            egui::ComboBox::from_id_salt("prefs_player_region")
                .width(260.0)
                .selected_text(label(prefs.region))
                .show_ui(ui, |ui| {
                    for region in 0..=8 {
                        let _ = ui.selectable_value(&mut prefs.region, region, label(region));
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label(tl!("Audio language:"));
            let _ = ui.add(
                egui::TextEdit::singleline(&mut prefs.audio_language)
                    .char_limit(3)
                    .desired_width(40.0),
            );
        });
        ui.horizontal(|ui| {
            ui.label(tl!("Subtitle language:"));
            let _ = ui.add(
                egui::TextEdit::singleline(&mut prefs.subtitle_language)
                    .char_limit(3)
                    .desired_width(40.0),
            );
        });
        ui.horizontal(|ui| {
            ui.label(tl!("Player parental level:"));
            let label = |level: u8| match level {
                0 => tl!("None").to_string(),
                level => level.to_string(),
            };
            egui::ComboBox::from_id_salt("prefs_player_parental")
                .selected_text(label(prefs.parental_level))
                .show_ui(ui, |ui| {
                    for level in 0..=8 {
                        let _ = ui.selectable_value(&mut prefs.parental_level, level, label(level));
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label(tl!("Preview playback quality:"));
            let mut quality = prefs.quality();
            egui::ComboBox::from_id_salt("prefs_preview_quality")
                .selected_text(quality.label())
                .show_ui(ui, |ui| {
                    for choice in crate::workspace::PlaybackQuality::ALL {
                        let _ = ui.selectable_value(&mut quality, choice, choice.label());
                    }
                });
            prefs.quality = (quality != crate::workspace::PlaybackQuality::High).then_some(quality);
        });
        ui.small(tl!(
            "The player the previews stand for. A disc that is not for its region, or whose parental level is above the player's, does not play."
        ));
        if prefs != self.preview_prefs {
            if let Err(error) = crate::workspace::save_preview_prefs(&prefs) {
                self.status = error;
            }
            self.preview_prefs = prefs;
        }
    }

    /// 「メニュー」（Encore と同じ並び）: ボタンのルートと、ボタン同期の初期設定。DiscCraft はプロジェクトに保存する
    /// （どのパソコンでビルドしても同じルートになるように）。
    fn prefs_menus(&mut self, ui: &mut Ui) {
        ui.strong(tl!("Menus"));
        ui.add_space(6.0);
        let settings = &self.session.project.settings;
        let mut routing = settings.routing;
        let mut defaults = settings.button_defaults;
        let mut change: Option<serde_json::Value> = None;
        ui.group(|ui| {
            ui.label(tl!("Button Routing"));
            for vertical in [true, false] {
                let (moves, wraps, mode) = if vertical {
                    (
                        &mut routing.vertical,
                        &mut routing.vertical_wrap,
                        &mut routing.vertical_wrap_mode,
                    )
                } else {
                    (
                        &mut routing.horizontal,
                        &mut routing.horizontal_wrap,
                        &mut routing.horizontal_wrap_mode,
                    )
                };
                let (move_label, wrap_label, key) = if vertical {
                    (
                        tl!("Route Buttons Up / Down"),
                        tl!("Wrap Up / Down"),
                        "vertical",
                    )
                } else {
                    (
                        tl!("Route Buttons Left / Right"),
                        tl!("Wrap Left / Right"),
                        "horizontal",
                    )
                };
                if ui.checkbox(moves, move_label).changed() {
                    change = Some(serde_json::json!({ key: *moves }));
                }
                ui.add_enabled_ui(*moves, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        if ui.checkbox(wraps, wrap_label).changed() {
                            change = Some(serde_json::json!({ format!("{key}Wrap"): *wraps }));
                        }
                        let name = |mode: disccraft_project::WrapMode| match (mode, vertical) {
                            (disccraft_project::WrapMode::SameLine, true) => {
                                tl!("Wrap within Same Column")
                            }
                            (disccraft_project::WrapMode::NextLine, true) => {
                                tl!("Wrap to Next Column")
                            }
                            (disccraft_project::WrapMode::SameLine, false) => {
                                tl!("Wrap within Same Row")
                            }
                            (disccraft_project::WrapMode::NextLine, false) => {
                                tl!("Wrap to Next Row")
                            }
                        };
                        ui.add_enabled_ui(*wraps, |ui| {
                            egui::ComboBox::from_id_salt(("prefs_wrap", key))
                                .width(180.0)
                                .selected_text(name(*mode))
                                .show_ui(ui, |ui| {
                                    for choice in [
                                        disccraft_project::WrapMode::SameLine,
                                        disccraft_project::WrapMode::NextLine,
                                    ] {
                                        if ui
                                            .selectable_label(*mode == choice, name(choice))
                                            .clicked()
                                            && *mode != choice
                                        {
                                            change = Some(serde_json::json!({
                                                format!("{key}WrapMode"): choice.as_str()
                                            }));
                                        }
                                    }
                                });
                        });
                    });
                });
            }
            ui.separator();
            if ui
                .checkbox(
                    &mut routing.exclude_next_previous,
                    tl!("Exclude Next and Previous Buttons"),
                )
                .changed()
            {
                change = Some(
                    serde_json::json!({ "excludeNextPrevious": routing.exclude_next_previous }),
                );
            }
        });
        ui.group(|ui| {
            ui.label(tl!("Button Sync Defaults"));
            if ui
                .checkbox(&mut defaults.sync_name, tl!("Sync Button Text and Name"))
                .changed()
            {
                change = Some(serde_json::json!({ "syncButtonNames": defaults.sync_name }));
            }
            if ui
                .checkbox(&mut defaults.name_from_link, tl!("Set Name from Link"))
                .changed()
            {
                change =
                    Some(serde_json::json!({ "nameButtonsFromLinks": defaults.name_from_link }));
            }
        });
        ui.small(tl!(
            "Saved with this project, so every computer builds the same routes."
        ));
        if let Some(params) = change {
            let _ = self.run_command("disc.setMenuOptions", params);
        }
    }

    /// 「表示方法」: 表示言語。
    fn prefs_interface(&mut self, ui: &mut Ui) {
        ui.strong(tl!("Interface"));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(tl!("Language:"));
            let mut chosen = None;
            egui::ComboBox::from_id_salt("prefs_language")
                .selected_text(self.language.name())
                .show_ui(ui, |ui| {
                    for language in Language::ALL {
                        if ui
                            .selectable_label(self.language == language, language.name())
                            .clicked()
                        {
                            chosen = Some(language);
                        }
                    }
                });
            if let Some(language) = chosen {
                self.set_language(language);
            }
        });
    }
}

fn standard_label(standard: VideoStandard) -> &'static str {
    match standard {
        VideoStandard::Ntsc => "NTSC",
        VideoStandard::Pal => "PAL",
    }
}
