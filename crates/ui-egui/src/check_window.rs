//! 「プロジェクトをチェック」の浮動ウィンドウ（Encore と同じ形。docs/ui-parity.md §3.5）。
//!
//! 上にチェックする項目のチェックボックスと「開始」、下に結果の表（種類 / 問題）と件数を出す。

use crate::DiscCraftApp;
use crate::build_panel::{ProblemView, problem_color};
use crate::i18n::{tl, tlf};

/// Kinds of problems the window can look for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CheckCategory {
    /// Button links (Encore: ボタンリンク): targets, links not set, buttons off the frame, the default button.
    ButtonLinks,
    /// First play.
    FirstPlay,
    /// Orphans: items nothing links to.
    Orphans,
    /// End actions.
    EndActions,
    /// Title button.
    TitleButton,
    /// Playlists and slideshows.
    Playlists,
    /// Overrides.
    Overrides,
    /// Remote menus.
    RemoteMenus,
    /// Overlapping buttons (and too many of them).
    OverlappingButtons,
    /// Menu total size: menus without buttons, durations, mixed aspect ratios, Blu-ray menu media.
    MenuSize,
    /// Disc capacity.
    Capacity,
    /// Chapters and trims.
    Chapters,
    /// Track and title limits (DiscCraft).
    StreamLimits,
    /// Media files (DiscCraft).
    Media,
    /// Other problems.
    Other,
}

impl CheckCategory {
    /// All categories, in Encore's order (three columns: ボタンリンク / ファーストプレイ / 孤立, エンドアクション /
    /// タイトルボタン / 再生リスト, 上書き / リモートメニュー / 重なったボタン, メニューの合計サイズ / ディスク容量 /
    /// チャプターおよびトリム), then DiscCraft's own.
    pub(crate) const ALL: [CheckCategory; 15] = [
        Self::ButtonLinks,
        Self::FirstPlay,
        Self::Orphans,
        Self::EndActions,
        Self::TitleButton,
        Self::Playlists,
        Self::Overrides,
        Self::RemoteMenus,
        Self::OverlappingButtons,
        Self::MenuSize,
        Self::Capacity,
        Self::Chapters,
        Self::StreamLimits,
        Self::Media,
        Self::Other,
    ];

    /// Checkbox label for this category.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::ButtonLinks => tl!("Button Links"),
            Self::FirstPlay => tl!("First play"),
            Self::Orphans => tl!("Orphans"),
            Self::EndActions => tl!("End actions"),
            Self::TitleButton => tl!("Title Button"),
            Self::Playlists => tl!("Playlists"),
            Self::Overrides => tl!("Overrides"),
            Self::RemoteMenus => tl!("Remote Menus"),
            Self::OverlappingButtons => tl!("Overlapping Buttons"),
            Self::MenuSize => tl!("Menu Total Size"),
            Self::Capacity => tl!("Disc capacity"),
            Self::Chapters => tl!("Chapters and Trims"),
            Self::StreamLimits => tl!("Track and title limits"),
            Self::Media => tl!("Media files"),
            Self::Other => tl!("Other"),
        }
    }

    /// Category of a problem code.
    #[must_use]
    pub(crate) fn of_code(code: &str) -> Self {
        match code {
            "missingTarget" | "buttonLinkNotSet" | "buttonOutOfFrame" | "badDefaultButton" => {
                Self::ButtonLinks
            }
            "firstPlayNotSet" => Self::FirstPlay,
            "unreachableMenu" | "unreachableTimeline" => Self::Orphans,
            "endActionNotSet" => Self::EndActions,
            "titleButtonNotMenu" => Self::TitleButton,
            "emptyPlaylist"
            | "emptySlideshow"
            | "badSlideDuration"
            | "emptyChapterPlaylist"
            | "tooManyEntries" => Self::Playlists,
            "menuRemoteNotMenu" => Self::RemoteMenus,
            "buttonsOverlap" | "tooManyButtons" => Self::OverlappingButtons,
            "menuWithoutButtons" | "menuDuration" | "menuAspectMix" => Self::MenuSize,
            "chapterOrder" | "firstChapter" | "tooManyChapters" => Self::Chapters,
            "assetOffline" | "missingAsset" | "unsupportedMedia" => Self::Media,
            "tooManyAudio" | "tooManySubtitles" | "tooManyTitles" => Self::StreamLimits,
            "capacityExceeded" | "lowBitrate" => Self::Capacity,
            _ => Self::Other,
        }
    }
}

/// State of the window.
#[derive(Clone, Debug)]
pub(crate) struct CheckWindow {
    /// The window is shown.
    pub(crate) open: bool,
    /// Which categories are checked, in `CheckCategory::ALL` order (all true by default).
    pub(crate) enabled: [bool; 15],
}

/// Default state for the check window.
impl Default for CheckWindow {
    /// Creates a closed window with all categories enabled.
    fn default() -> Self {
        Self {
            open: false,
            enabled: [true; 15],
        }
    }
}

impl CheckWindow {
    /// Whether problems of `category` are shown.
    #[must_use]
    pub(crate) fn shows(&self, category: CheckCategory) -> bool {
        CheckCategory::ALL
            .iter()
            .zip(self.enabled.iter())
            .any(|(&cat, &enabled)| cat == category && enabled)
    }
}

/// Problems whose category is enabled.
#[must_use]
pub(crate) fn visible_problems<'a>(
    window: &CheckWindow,
    problems: &'a [ProblemView],
) -> Vec<&'a ProblemView> {
    problems
        .iter()
        .filter(|problem| window.shows(CheckCategory::of_code(&problem.code)))
        .collect()
}

/// Severity text for the table.
#[must_use]
pub(crate) fn severity_label(severity: &str) -> &'static str {
    // Same matching as `problem_color` (the engine may send "Error" or "error").
    let lower = severity.to_lowercase();
    if lower.contains("error") {
        tl!("Error")
    } else if lower.contains("warn") {
        tl!("Warning")
    } else {
        tl!("Info")
    }
}

impl DiscCraftApp {
    /// Draw the window when open.
    pub(crate) fn check_window(&mut self, ctx: &egui::Context) {
        let mut open = self.check_window.open;

        let _ = egui::Window::new(tl!("Check Project"))
            .id(egui::Id::unique("check_window"))
            .open(&mut open)
            .resizable(true)
            .default_size([600.0, 380.0])
            .default_pos([380.0, 140.0])
            .show(ctx, |ui| {
                self.draw_check_window(ui);
            });

        self.check_window.open = open;
    }

    /// Draw the contents of the check window.
    fn draw_check_window(&mut self, ui: &mut egui::Ui) {
        let _ = ui.horizontal(|ui| {
            self.draw_category_group(ui);
            self.draw_start_button(ui);
        });

        self.draw_results(ui);
    }

    /// Draw the category checkbox group.
    fn draw_category_group(&mut self, ui: &mut egui::Ui) {
        let enabled = &mut self.check_window.enabled;

        let _ = ui.group(|ui| {
            let _ = ui.label(tl!("Check for"));
            Self::draw_category_grid(ui, enabled);
        });
    }

    /// Draw the category checkboxes in a three-column grid.
    fn draw_category_grid(ui: &mut egui::Ui, enabled: &mut [bool; 15]) {
        let _ = egui::Grid::new("check_categories")
            .num_columns(3)
            .show(ui, |ui| {
                for (index, (category, enabled)) in CheckCategory::ALL
                    .into_iter()
                    .zip(enabled.iter_mut())
                    .enumerate()
                {
                    let _ = ui.checkbox(enabled, category.label());

                    if (index + 1) % 3 == 0 {
                        ui.end_row();
                    }
                }
            });
    }

    /// Draw the start button.
    fn draw_start_button(&mut self, ui: &mut egui::Ui) {
        let _ = ui.vertical(|ui| {
            if ui
                .add_sized([88.0, 28.0], egui::Button::new(tl!("Start")))
                .clicked()
            {
                self.check_project();
            }
        });
    }

    /// Draw the results table and footer.
    fn draw_results(&self, ui: &mut egui::Ui) {
        let visible = visible_problems(&self.check_window, &self.problems);
        let count = visible.len();

        let _ = egui::ScrollArea::vertical()
            .max_height(220.0)
            .show(ui, |ui| {
                Self::draw_results_grid(ui, &visible);
            });

        let _ = ui.label(tlf!("{count} items found.", count = count));
    }

    /// Draw the results grid.
    fn draw_results_grid(ui: &mut egui::Ui, visible: &[&ProblemView]) {
        let _ = crate::theme::table("check_results")
            .num_columns(2)
            .show(ui, |ui| {
                let _ = ui.strong(tl!("Type"));
                let _ = ui.strong(tl!("Problem"));
                ui.end_row();

                for problem in visible {
                    let color = problem_color(&problem.severity);
                    let _ = ui.colored_label(color, severity_label(&problem.severity));
                    let _ = ui.label(problem.message.as_str());
                    ui.end_row();
                }
            });
    }
}

/// Tests for the check window.
#[cfg(test)]
mod tests {
    use super::{CheckCategory, CheckWindow, severity_label, visible_problems};
    use crate::build_panel::ProblemView;
    use crate::i18n::{Language, set_current};

    /// Maps one code for every category and an unknown code.
    #[test]
    fn of_code_maps_each_category() {
        assert_eq!(
            CheckCategory::of_code("missingTarget"),
            CheckCategory::ButtonLinks
        );
        assert_eq!(
            CheckCategory::of_code("endActionNotSet"),
            CheckCategory::EndActions
        );
        assert_eq!(
            CheckCategory::of_code("firstPlayNotSet"),
            CheckCategory::FirstPlay
        );
        assert_eq!(
            CheckCategory::of_code("unreachableMenu"),
            CheckCategory::Orphans
        );
        assert_eq!(
            CheckCategory::of_code("chapterOrder"),
            CheckCategory::Chapters
        );
        assert_eq!(CheckCategory::of_code("assetOffline"), CheckCategory::Media);
        assert_eq!(
            CheckCategory::of_code("tooManyAudio"),
            CheckCategory::StreamLimits
        );
        assert_eq!(
            CheckCategory::of_code("capacityExceeded"),
            CheckCategory::Capacity
        );
        assert_eq!(
            CheckCategory::of_code("buttonsOverlap"),
            CheckCategory::OverlappingButtons
        );
        assert_eq!(
            CheckCategory::of_code("menuWithoutButtons"),
            CheckCategory::MenuSize
        );
        assert_eq!(
            CheckCategory::of_code("emptySlideshow"),
            CheckCategory::Playlists
        );
        assert_eq!(CheckCategory::of_code("unknown"), CheckCategory::Other);
    }

    /// Hides problems from disabled categories.
    #[test]
    fn visible_problems_hides_disabled_category() {
        let mut window = CheckWindow::default();

        for (enabled, category) in window.enabled.iter_mut().zip(CheckCategory::ALL) {
            *enabled = category != CheckCategory::ButtonLinks;
        }

        let problems = vec![
            ProblemView {
                severity: "error".to_owned(),
                code: "missingTarget".to_owned(),
                message: "link".to_owned(),
            },
            ProblemView {
                severity: "warning".to_owned(),
                code: "unknown".to_owned(),
                message: "other".to_owned(),
            },
        ];

        let visible = visible_problems(&window, &problems);

        assert_eq!(visible.len(), 1);
        assert!(visible.iter().any(|problem| problem.message == "other"));
    }

    /// Uses the current language for severity labels.
    #[test]
    fn severity_label_uses_current_language() {
        set_current(Language::En);

        assert_eq!(severity_label("error"), "Error");
        assert_eq!(severity_label("warning"), "Warning");
        assert_eq!(severity_label("info"), "Info");

        set_current(Language::Ja);
    }

    /// Default window is closed and has every category enabled.
    #[test]
    fn default_window_is_closed_with_all_categories_enabled() {
        let window = CheckWindow::default();

        assert!(!window.open);
        assert!(window.enabled.iter().all(|enabled| *enabled));
    }
}
