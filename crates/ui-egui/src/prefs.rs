//! 環境設定ダイアログ（Encore と同じ形。編集 ▸ 環境設定、Ctrl+K。[docs/ui-parity.md](../../../docs/ui-parity.md) §3.5）。
//!
//! 左に分類の一覧、右にその設定を置く。「一般」はテレビ方式の初期設定（新規プロジェクトの既定）、
//! 「表示方法」は表示言語。設定は `config.toml` に保存する（M9.62。前は `workspace.json`）。
//!
//! M6.512: 並べ方はほかの Craft アプリの環境設定（FilmCraft）と同じ: 角の丸い枠と上の題名の帯、左は枠のある一覧で
//! 選んだ行を行の幅いっぱいに塗る、右はラベルを右寄せにした列とそろった入力欄、説明は薄く小さい文字で入力欄の列に、
//! まとまりは題名を枠に入れたグループ、長いページはスクロール。分類と項目は Encore のまま。

use crate::DiscCraftApp;
use crate::i18n::{Language, tl, tlf};
use crate::theme::Shade;

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

/// The dialog's width (M6.512: laid out as the other Craft apps' Preferences, FilmCraft's).
const DIALOG_WIDTH: f32 = 760.0;
/// The height of the category list and of the page beside it (a longer page scrolls).
const BODY_HEIGHT: f32 = 330.0;
/// The category list's width.
const LIST_WIDTH: f32 = 176.0;
/// A category's row in the list.
const LIST_ROW: f32 = 24.0;
/// The column of the field labels (right-aligned in it), so that every field starts at the same place.
const LABEL_WIDTH: f32 = 190.0;
/// The width of a field's choice list.
const CHOICE_WIDTH: f32 = 240.0;
/// The width of a choice list of long names (devices, regions).
const WIDE_CHOICE_WIDTH: f32 = 320.0;
/// The space around the dialog's body.
const SIDE: f32 = 20.0;

/// The title bar's ground.
const TITLE_BAR: Shade = Shade::gray(50, 226);
/// The category list's ground, its selected row and a row under the pointer.
const LIST_GROUND: Shade = Shade::gray(46, 250);
const LIST_HOVER: Shade = Shade::gray(64, 236);
/// The edge of a group of settings.
const GROUP_EDGE: Shade = Shade::gray(92, 196);

/// A setting's row: its label right-aligned in the label column, then what sets it.
fn field<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let width = LABEL_WIDTH.min(ui.available_width() * 0.45);
        let _ = ui.allocate_ui_with_layout(
            egui::vec2(width, 22.0),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                if !label.is_empty() {
                    ui.add(egui::Label::new(label).wrap());
                }
            },
        );
        add(ui)
    })
    .inner
}

/// What a setting does, in small dim type under it, in the column of the fields.
fn note(ui: &mut Ui, text: impl Into<String>) {
    ui.horizontal_top(|ui| {
        let width = LABEL_WIDTH.min(ui.available_width() * 0.45);
        ui.add_space(width + ui.spacing().item_spacing.x);
        ui.vertical(|ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(text.into())
                        .small()
                        .color(crate::theme::TEXT_WEAK.get()),
                )
                .wrap(),
            );
        });
    });
    ui.add_space(6.0);
}

/// A group of settings: a thin rounded edge with its title set into the top edge (as FilmCraft's Preferences).
fn group(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    ui.add_space(10.0);
    let framed = egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, GROUP_EDGE.get()))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin {
            left: 12,
            right: 12,
            top: 14,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
    let edge = framed.response.rect;
    let font = egui::TextStyle::Body.resolve(ui.style());
    let color = crate::theme::TEXT_WEAK.get();
    let galley = ui.painter().layout_no_wrap(title.to_owned(), font, color);
    let at = edge.left_top() + egui::vec2(10.0, -galley.size().y / 2.0);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(
            at - egui::vec2(4.0, 0.0),
            galley.size() + egui::vec2(8.0, 0.0),
        ),
        0.0,
        ui.visuals().window_fill,
    );
    ui.painter().galley(at, galley, color);
    ui.add_space(2.0);
}

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

        // As FilmCraft's Preferences (M6.512): a dialog with round corners in the middle of the window, over the
        // dimmed interface, its title in a bar on top, the categories in a list on the left and the page on the
        // right. The settings take effect as they are changed, so the one button closes it (Esc and a click
        // outside do too).
        let mut close = false;
        let visuals = ctx.global_style().visuals.clone();
        let frame = egui::Frame::new()
            .fill(visuals.window_fill)
            .stroke(visuals.window_stroke)
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::ZERO);
        let response = egui::Modal::new(Id::unique("prefs_window"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(DIALOG_WIDTH);
                egui::Frame::new()
                    .fill(TITLE_BAR.get())
                    .corner_radius(egui::CornerRadius {
                        nw: 10,
                        ne: 10,
                        sw: 0,
                        se: 0,
                    })
                    .inner_margin(egui::Margin::symmetric(0, 7))
                    .show(ui, |ui| {
                        ui.set_width(DIALOG_WIDTH);
                        ui.vertical_centered(|ui| ui.strong(tl!("Preferences")));
                    });
                ui.add_space(14.0);
                self.prefs_contents(ui);
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(SIDE);
                        if ui.add(crate::theme::default_button(tl!("Close"))).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(14.0);
            });
        if close || response.should_close() {
            self.prefs.open = false;
        }
    }

    /// 左の分類と右の設定。
    fn prefs_contents(&mut self, ui: &mut Ui) {
        ui.horizontal_top(|ui| {
            ui.add_space(SIDE);
            self.prefs_categories(ui);
            ui.add_space(SIDE);
            let width = DIALOG_WIDTH - LIST_WIDTH - 3.0 * SIDE;
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.set_height(BODY_HEIGHT);
                egui::ScrollArea::vertical()
                    .id_salt(("prefs_page", self.prefs.category as u8))
                    .auto_shrink([false, false])
                    .max_height(BODY_HEIGHT)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 5.0;
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
        });
    }

    /// The category list (M6.512, as FilmCraft's): a framed list whose rows fill its width, the chosen one shaded.
    fn prefs_categories(&mut self, ui: &mut Ui) {
        let (list, _) =
            ui.allocate_exact_size(egui::vec2(LIST_WIDTH, BODY_HEIGHT), egui::Sense::hover());
        ui.painter().rect(
            list,
            3.0,
            LIST_GROUND.get(),
            egui::Stroke::new(1.0, crate::theme::BORDER.get()),
            egui::StrokeKind::Inside,
        );
        let font = egui::TextStyle::Body.resolve(ui.style());
        let mut top = list.min.y + 1.0;
        for category in PrefsCategory::ALL {
            let row = egui::Rect::from_min_size(
                egui::pos2(list.min.x + 1.0, top),
                egui::vec2(LIST_WIDTH - 2.0, LIST_ROW),
            );
            top += LIST_ROW;
            let response = ui.interact(
                row,
                Id::unique(("prefs_category", category as u8)),
                egui::Sense::click(),
            );
            let chosen = self.prefs.category == category;
            if chosen {
                ui.painter()
                    .rect_filled(row, 0.0, crate::theme::SELECTION_ROW.get());
            } else if response.hovered() {
                ui.painter().rect_filled(row, 0.0, LIST_HOVER.get());
            }
            let color = if chosen {
                crate::theme::TEXT_STRONG.get()
            } else {
                crate::theme::TEXT.get()
            };
            let galley =
                ui.painter()
                    .layout_no_wrap(category.label().to_owned(), font.clone(), color);
            ui.painter().galley(
                row.left_center() + egui::vec2(10.0, -galley.size().y / 2.0),
                galley,
                color,
            );
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::Role::ListItem,
                    ui.is_enabled(),
                    chosen,
                    category.label(),
                )
            });
            if response.clicked() {
                self.prefs.category = category;
            }
        }
    }

    /// 「一般」: テレビ方式の初期設定（新規プロジェクトの既定）。
    fn prefs_general(&mut self, ui: &mut Ui) {
        field(ui, tl!("Default TV standard:"), |ui| {
            let current = crate::workspace::load_default_standard();
            let mut chosen = None;
            egui::ComboBox::from_id_salt("prefs_standard")
                .width(CHOICE_WIDTH)
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
        note(ui, tl!("Used as the TV standard of new projects."));
        field(ui, tl!("Playback quality:"), |ui| {
            let mut quality = self.playback_quality;
            egui::ComboBox::from_id_salt("prefs_playback_quality")
                .width(CHOICE_WIDTH)
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
        note(
            ui,
            tl!(
                "How large the Monitor plays a title's pictures: High at full size, Draft at half, Automatic at half only for HD larger than 1280 × 720."
            ),
        );
        // M9.65: off until the user turns it on; Help ▸ Check for Updates… asks at any time.
        let mut at_start = crate::workspace::load_check_updates_at_start();
        let toggled = field(ui, "", |ui| {
            ui.checkbox(&mut at_start, tl!("Check for updates at startup"))
                .changed()
        });
        note(
            ui,
            tl!(
                "Asks GitHub for a newer release each time DiscCraft starts. Off: DiscCraft asks only when you choose Help ▸ Check for Updates…."
            ),
        );
        if toggled {
            match crate::workspace::save_check_updates_at_start(at_start) {
                Ok(()) => self.updates.set_startup_check(at_start),
                Err(error) => self.status = error,
            }
        }
    }

    /// 「タイムライン」（Encore: 静止画デュレーション / 字幕デュレーション / オーディオの言語 / 字幕の言語）。
    fn prefs_timelines(&mut self, ui: &mut Ui) {
        let mut prefs = self.timeline_prefs.clone();
        field(ui, tl!("Still duration:"), |ui| {
            let _ = ui.add(
                egui::DragValue::new(&mut prefs.still_seconds)
                    .range(1..=3600)
                    .suffix(tl!(" s")),
            );
        });
        note(ui, tl!("The slide duration of new slideshows."));
        field(ui, tl!("Subtitle duration:"), |ui| {
            let _ = ui.add(
                egui::DragValue::new(&mut prefs.subtitle_seconds)
                    .range(1..=60)
                    .suffix(tl!(" s")),
            );
        });
        note(ui, tl!("The length of a subtitle added in the Monitor."));
        field(ui, tl!("Audio language:"), |ui| {
            let _ = ui.add(
                egui::TextEdit::singleline(&mut prefs.audio_language)
                    .char_limit(3)
                    .desired_width(48.0),
            );
        });
        field(ui, tl!("Subtitle language:"), |ui| {
            let _ = ui.add(
                egui::TextEdit::singleline(&mut prefs.subtitle_language)
                    .char_limit(3)
                    .desired_width(48.0),
            );
        });
        note(
            ui,
            tl!(
                "The language of new tracks (a code such as ja or en). Empty uses the disc's audio language."
            ),
        );
        field(ui, "", |ui| {
            let _ = ui.checkbox(&mut prefs.snap_subtitles, tl!("Snap subtitle clips"));
        });
        note(
            ui,
            tl!(
                "A subtitle clip dragged in a timeline snaps to the edges of the subtitle clips, the chapter points and the playhead near it."
            ),
        );
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
        let current = crate::workspace::load_audio_device();
        let names = crate::audio_out::device_names();
        let mut chosen: Option<Option<String>> = None;
        field(ui, tl!("Audio output device:"), |ui| {
            egui::ComboBox::from_id_salt("prefs_audio_device")
                .width(WIDE_CHOICE_WIDTH)
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
        note(
            ui,
            tl!(
                "Where playback in the Monitor and the previews sounds, from the next time it starts. A device that is no longer there gives way to the system's default."
            ),
        );
        if let Some(name) = chosen {
            match crate::workspace::save_audio_device(name.as_deref()) {
                Ok(()) => crate::audio_out::choose_device(name),
                Err(error) => self.status = error,
            }
        }

        // Video output (Encore: Video Devices, a tick for each connected monitor).
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
        field(ui, tl!("Video output:"), |ui| {
            egui::ComboBox::from_id_salt("prefs_video_output")
                .width(WIDE_CHOICE_WIDTH)
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
        let toggled = field(ui, "", |ui| {
            ui.checkbox(
                &mut background_off,
                tl!("Disable video output when in the background"),
            )
            .changed()
        });
        note(
            ui,
            tl!(
                "The picture of the title the Monitor or a preview plays, shown full screen on another display or in a window of its own. Esc in that window turns it off."
            ),
        );
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
        // One place, in a group of its own: where it is, Browse… to choose another, Default to go back, Show in
        // Explorer (and `more`, the group's other buttons). Returns the change asked for: `Some(None)` is back to
        // the default. Browse… asks for the folder, which comes to `browse` on a later frame (M9.64).
        let folder_group = |app: &mut Self,
                            ui: &mut Ui,
                            title: &str,
                            (folder, is_default): (Option<std::path::PathBuf>, bool),
                            browse: crate::file_dialogs::Then,
                            more: &mut dyn FnMut(&mut Self, &mut Ui)|
         -> Option<Option<std::path::PathBuf>> {
            let mut change = None;
            // In the group the place and its buttons start at its left edge: the label column would leave no room
            // for four buttons.
            group(ui, title, |ui| {
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
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add(crate::theme::dialog_button(tl!("Browse…")))
                        .clicked()
                    {
                        let mut dialog = craft_dialogs::FileDialog::folder();
                        if let Some(folder) = folder.as_ref().filter(|folder| folder.is_dir()) {
                            dialog = dialog.directory(folder);
                        }
                        crate::file_dialogs::ask(dialog, browse.clone());
                    }
                    if ui
                        .add_enabled(!is_default, crate::theme::dialog_button(tl!("Default")))
                        .clicked()
                    {
                        change = Some(None);
                    }
                    if ui
                        .add_enabled(
                            folder.as_ref().is_some_and(|folder| folder.is_dir()),
                            crate::theme::dialog_button(tl!("Show in Explorer")),
                        )
                        .clicked()
                        && let Some(folder) = &folder
                    {
                        app.reveal_in_file_manager(folder);
                    }
                    more(app, ui);
                });
            });
            change
        };

        let library = crate::workspace::library_dir();
        let default = crate::workspace::default_library_dir();
        let is_default = library == default;
        if let Some(chosen) = folder_group(
            self,
            ui,
            tl!("Library Content"),
            (library, is_default),
            crate::file_dialogs::Then::LibraryContentFolder,
            &mut |_, _| {},
        ) {
            self.move_library(chosen.as_deref());
        }
        let cache = crate::workspace::media_cache_dir();
        let default = crate::workspace::default_media_cache_dir();
        let is_default = cache == default;
        let has_cache = cache.is_some();
        let mut clean = |app: &mut Self, ui: &mut Ui| {
            if ui
                .add_enabled(has_cache, crate::theme::dialog_button(tl!("Clean")))
                .on_hover_text(tl!(
                    "Forgets the lengths, sizes and thumbnails remembered of media files; they are read again when needed"
                ))
                .clicked()
            {
                let removed = disccraft_engine::clear_media_cache();
                app.durations.clear();
                app.thumbnails.clear();
                app.status = tlf!("Media cache cleaned: {n} files", n = removed);
            }
        };
        if let Some(chosen) = folder_group(
            self,
            ui,
            tl!("Media Cache Database"),
            (cache, is_default),
            crate::file_dialogs::Then::MediaCacheFolder,
            &mut clean,
        ) {
            self.set_media_cache_dir(chosen.as_deref());
        }
    }

    /// Keeps what is learnt of media files in `folder` (`None`: the default place).
    pub(crate) fn set_media_cache_dir(&mut self, folder: Option<&std::path::Path>) {
        match crate::workspace::save_media_cache_dir(folder) {
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

    /// Puts the library content in `folder` (`None`: back beside the settings). A folder that has a library
    /// already is opened as it is; an empty one gets the items the library has now.
    pub(crate) fn move_library(&mut self, folder: Option<&std::path::Path>) {
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
        let installed = disccraft_platform::installed_memory_bytes().map_or_else(
            || tl!("Unknown").to_string(),
            // `as`: a size shown to one decimal.
            |bytes| format!("{:.1} GiB", bytes as f64 / (1u64 << 30) as f64),
        );
        field(ui, tl!("Installed RAM:"), |ui| {
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
        field(ui, tl!("Transcodes at a time:"), |ui| {
            egui::ComboBox::from_id_salt("prefs_parallel_transcodes")
                .width(CHOICE_WIDTH)
                .selected_text(label(current))
                .show_ui(ui, |ui| {
                    for most in 0..=cores {
                        if ui.selectable_label(current == most, label(most)).clicked() {
                            chosen = Some(most);
                        }
                    }
                });
        });
        note(
            ui,
            tl!(
                "A build and Transcode Now convert several titles at once, one for every processor core. Each takes memory of its own: choose fewer to leave more for other applications."
            ),
        );
        if let Some(most) = chosen {
            match crate::workspace::save_parallel_transcodes(most) {
                Ok(()) => {
                    disccraft_build::set_parallel_transcodes(usize::try_from(most).unwrap_or(0));
                }
                Err(error) => self.status = error,
            }
        }
        // M12.40: HD H.264 on the system's hardware encoder.
        let mut hardware = crate::workspace::load_hardware_encoder();
        let encoders = disccraft_platform::hardware_h264_encoders();
        let toggled = field(ui, "", |ui| {
            ui.add_enabled(
                !encoders.is_empty(),
                egui::Checkbox::new(&mut hardware, tl!("Use the hardware encoder for HD H.264")),
            )
            .on_disabled_hover_text(tl!("This computer has no hardware H.264 encoder"))
            .changed()
        });
        note(
            ui,
            match encoders.first() {
                Some(name) => tlf!(
                    "{name}: the Blu-ray's 1080p and 720p H.264 in one pass (VBR or CBR), several times as fast. 1080i, standard definition and two-pass encodes stay on the processor, as does a video the encoder fails on.",
                    name = name
                ),
                None => tl!("No hardware H.264 encoder was found.").to_string(),
            },
        );
        if toggled {
            match crate::workspace::save_hardware_encoder(hardware) {
                Ok(()) => disccraft_platform::use_for_transcodes(hardware),
                Err(error) => self.status = error,
            }
        }
    }

    /// "Preview" (Encore: TV Mode / Player Region Code / Audio Language; the parental level is DiscCraft's): the
    /// player the Project Preview and the Monitor stand for.
    fn prefs_preview(&mut self, ui: &mut Ui) {
        let mut prefs = self.preview_prefs.clone();
        field(ui, tl!("TV mode:"), |ui| {
            // (4:3 television, pan and scan).
            let label = |mode: (bool, bool)| match mode {
                (false, _) => tl!("16:9 Widescreen"),
                (true, false) => tl!("4:3 Letterbox"),
                (true, true) => tl!("4:3 Pan & Scan"),
            };
            let mut mode = (prefs.tv_4x3, prefs.pan_scan);
            egui::ComboBox::from_id_salt("prefs_tv_mode")
                .width(CHOICE_WIDTH)
                .selected_text(label(mode))
                .show_ui(ui, |ui| {
                    for choice in [(false, false), (true, false), (true, true)] {
                        let _ = ui.selectable_value(&mut mode, choice, label(choice));
                    }
                });
            (prefs.tv_4x3, prefs.pan_scan) = mode;
        });
        field(ui, tl!("Player region code:"), |ui| {
            let label = |region: u8| match region {
                0 => tl!("All regions"),
                region => crate::build_view::region_label(region),
            };
            egui::ComboBox::from_id_salt("prefs_player_region")
                .width(WIDE_CHOICE_WIDTH)
                .selected_text(label(prefs.region))
                .show_ui(ui, |ui| {
                    for region in 0..=8 {
                        let _ = ui.selectable_value(&mut prefs.region, region, label(region));
                    }
                });
        });
        field(ui, tl!("Audio language:"), |ui| {
            let _ = ui.add(
                egui::TextEdit::singleline(&mut prefs.audio_language)
                    .char_limit(3)
                    .desired_width(48.0),
            );
        });
        field(ui, tl!("Subtitle language:"), |ui| {
            let _ = ui.add(
                egui::TextEdit::singleline(&mut prefs.subtitle_language)
                    .char_limit(3)
                    .desired_width(48.0),
            );
        });
        field(ui, tl!("Player parental level:"), |ui| {
            let label = |level: u8| match level {
                0 => tl!("None").to_string(),
                level => level.to_string(),
            };
            egui::ComboBox::from_id_salt("prefs_player_parental")
                .width(CHOICE_WIDTH)
                .selected_text(label(prefs.parental_level))
                .show_ui(ui, |ui| {
                    for level in 0..=8 {
                        let _ = ui.selectable_value(&mut prefs.parental_level, level, label(level));
                    }
                });
        });
        field(ui, tl!("Preview playback quality:"), |ui| {
            let mut quality = prefs.quality();
            egui::ComboBox::from_id_salt("prefs_preview_quality")
                .width(CHOICE_WIDTH)
                .selected_text(quality.label())
                .show_ui(ui, |ui| {
                    for choice in crate::workspace::PlaybackQuality::ALL {
                        let _ = ui.selectable_value(&mut quality, choice, choice.label());
                    }
                });
            prefs.quality = (quality != crate::workspace::PlaybackQuality::High).then_some(quality);
        });
        note(
            ui,
            tl!(
                "The player the previews stand for. A disc that is not for its region, or whose parental level is above the player's, does not play."
            ),
        );
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
        let settings = &self.session.project.settings;
        let mut routing = settings.routing;
        let mut defaults = settings.button_defaults;
        let mut change: Option<serde_json::Value> = None;
        group(ui, tl!("Button Routing"), |ui| {
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
                        ui.add_space(22.0);
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
                                .width(CHOICE_WIDTH)
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
                ui.add_space(4.0);
            }
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
        group(ui, tl!("Button Sync Defaults"), |ui| {
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
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(
                egui::RichText::new(tl!(
                    "Saved with this project, so every computer builds the same routes."
                ))
                .small()
                .color(crate::theme::TEXT_WEAK.get()),
            )
            .wrap(),
        );
        if let Some(params) = change {
            let _ = self.run_command("disc.setMenuOptions", params);
        }
    }

    /// 「表示方法」: 表示言語。
    fn prefs_interface(&mut self, ui: &mut Ui) {
        field(ui, tl!("Language:"), |ui| {
            let mut chosen = None;
            egui::ComboBox::from_id_salt("prefs_language")
                .width(CHOICE_WIDTH)
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
        // Encore's Brightness (its slider from dark to light): the dark look, as Encore starts, or the light one
        // (M6.480).
        field(ui, tl!("Brightness:"), |ui| {
            let current = if crate::theme::is_light() {
                crate::theme::Appearance::Light
            } else {
                crate::theme::Appearance::Dark
            };
            let mut chosen = None;
            egui::ComboBox::from_id_salt("prefs_brightness")
                .width(CHOICE_WIDTH)
                .selected_text(current.label())
                .show_ui(ui, |ui| {
                    for appearance in crate::theme::Appearance::ALL {
                        if ui
                            .selectable_label(current == appearance, appearance.label())
                            .clicked()
                        {
                            chosen = Some(appearance);
                        }
                    }
                });
            if let Some(appearance) = chosen.filter(|chosen| *chosen != current) {
                self.set_appearance(ui.ctx(), appearance);
            }
        });
    }

    /// 画面の明るさを切り替えて設定に保存する（次のフレームから新しい明るさで描く）。
    pub(crate) fn set_appearance(&mut self, ctx: &Context, appearance: crate::theme::Appearance) {
        crate::theme::set_appearance(ctx, appearance);
        self.status = tlf!("Brightness: {name}", name = appearance.label());
        if let Err(error) = crate::workspace::save_appearance(appearance) {
            self.status = tlf!(
                "Brightness: {name} (could not save: {error})",
                name = appearance.label(),
                error
            );
        }
    }
}

fn standard_label(standard: VideoStandard) -> &'static str {
    match standard {
        VideoStandard::Ntsc => "NTSC",
        VideoStandard::Pal => "PAL",
    }
}
