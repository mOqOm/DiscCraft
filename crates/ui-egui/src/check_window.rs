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
    /// Timelines whose streams are over the disc's bit rate (Encore: 高すぎるタイムラインビットレート).
    TimelineBitrate,
    /// Subtitles whose text does not fit (Encore: 字幕テキストのオーバーフロー).
    SubtitleOverflow,
    /// Subtitles too close to the one before on a Blu-ray disc (Encore: 字幕の間隔).
    SubtitleSpacing,
    /// A chapter playlist's random or shuffle play that the disc cannot give in full (Encore: 再生の種類, M6.337).
    PlaybackType,
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
    /// チャプターおよびトリム, 高すぎるタイムラインビットレート / 字幕テキストのオーバーフロー / 字幕の間隔 / 再生の種類),
    /// then DiscCraft's own.
    pub(crate) const ALL: [CheckCategory; 19] = [
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
        Self::TimelineBitrate,
        Self::SubtitleOverflow,
        Self::SubtitleSpacing,
        Self::PlaybackType,
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
            Self::TimelineBitrate => tl!("Timeline Bitrate Too High"),
            Self::SubtitleOverflow => tl!("Subtitle Text Overflow"),
            Self::SubtitleSpacing => tl!("Subtitle Spacing"),
            Self::PlaybackType => tl!("Playback Type"),
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
            "timelineBitrateTooHigh" => Self::TimelineBitrate,
            "subtitleTextOverflow" => Self::SubtitleOverflow,
            "subtitleSpacing" => Self::SubtitleSpacing,
            "playbackType" => Self::PlaybackType,
            "assetOffline" | "missingAsset" | "unsupportedMedia" | "undecodableVideo" => {
                Self::Media
            }
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
    pub(crate) enabled: [bool; 19],
}

/// Default state for the check window.
impl Default for CheckWindow {
    /// Creates a closed window with all categories enabled.
    fn default() -> Self {
        Self {
            open: false,
            enabled: [true; 19],
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
    fn draw_category_grid(ui: &mut egui::Ui, enabled: &mut [bool; 19]) {
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
        let rows = check_rows(&self.session.project, &visible);
        let count = rows.len();

        let _ = egui::ScrollArea::vertical()
            .max_height(220.0)
            .show(ui, |ui| {
                Self::draw_results_grid(ui, &rows);
            });

        let _ = ui.label(tlf!("{count} items found.", count = count));
    }

    /// Draw the results grid: the item's name and its problem; several problems fold under "n errors" (Encore).
    fn draw_results_grid(ui: &mut egui::Ui, rows: &[CheckRow]) {
        let _ = crate::theme::table("check_results")
            .num_columns(2)
            .show(ui, |ui| {
                let _ = ui.strong(tl!("Name"));
                let _ = ui.strong(tl!("Problem"));
                ui.end_row();

                for (index, row) in rows.iter().enumerate() {
                    let _ = ui.label(row.name.as_str());
                    match row.problems.as_slice() {
                        [(severity, text)] => {
                            let _ = ui
                                .colored_label(problem_color(severity), text.as_str())
                                .on_hover_text(severity_label(severity));
                        }
                        problems => {
                            let count = problems.len();
                            let _ =
                                egui::CollapsingHeader::new(tlf!("{count} errors", count = count))
                                    .id_salt(("check_row", index))
                                    .show(ui, |ui| {
                                        for (severity, text) in problems {
                                            let _ = ui
                                                .colored_label(
                                                    problem_color(severity),
                                                    text.as_str(),
                                                )
                                                .on_hover_text(severity_label(severity));
                                        }
                                    });
                        }
                    }
                    ui.end_row();
                }
            });
    }
}

/// One line of the results: an item of the project and what is wrong with it (Encore: 名前 / 問題, one line per item).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CheckRow {
    /// The item's name (the disc for problems that belong to no item).
    pub(crate) name: String,
    /// Severity and text of each problem.
    pub(crate) problems: Vec<(String, String)>,
}

/// The item a problem message starts with: its kind, its id, and the button's index for `menu N button M`.
fn problem_subject(message: &str) -> Option<(&'static str, u64, Option<usize>)> {
    const KINDS: [&str; 6] = [
        "chapter playlist",
        "timeline",
        "menu",
        "playlist",
        "slideshow",
        "asset",
    ];
    let (kind, rest) = KINDS.iter().find_map(|kind| {
        let rest = message.strip_prefix(kind)?.strip_prefix(' ')?;
        Some((*kind, rest))
    })?;
    let mut words = rest.split(' ');
    let id = words.next()?.parse().ok()?;
    let button = match (kind, words.next(), words.next()) {
        ("menu", Some("button"), Some(index)) => index.parse().ok(),
        _ => None,
    };
    Some((kind, id, button))
}

/// The name of the item a problem is about (`None`: the disc, or an item that no longer exists).
fn subject_name(project: &disccraft_project::Project, message: &str) -> Option<String> {
    let (kind, id, button) = problem_subject(message)?;
    match kind {
        "timeline" => project
            .timeline(disccraft_project::TimelineId(id))
            .map(|timeline| timeline.name.clone()),
        "menu" => {
            let menu = project.menu(disccraft_project::MenuId(id))?;
            Some(match button.and_then(|index| menu.buttons.get(index)) {
                Some(button) => format!("{}: {}", menu.name, button.name),
                None => menu.name.clone(),
            })
        }
        "playlist" => project.playlist(id).map(|playlist| playlist.name.clone()),
        "slideshow" => project
            .slideshow(id)
            .map(|slideshow| slideshow.name.clone()),
        "chapter playlist" => project
            .chapter_playlist(id)
            .map(|playlist| playlist.name.clone()),
        "asset" => project
            .assets
            .get(&disccraft_project::AssetId(id))
            .map(|asset| asset.name.clone()),
        _ => None,
    }
}

/// The text of a problem: Encore's wording for the problems Encore reports, the engine's message for the rest.
fn problem_text(problem: &ProblemView) -> String {
    match problem.code.as_str() {
        "buttonLinkNotSet" => tl!("The link is not set.").to_owned(),
        "endActionNotSet" => tl!("The end action is not set.").to_owned(),
        "unreachableTimeline" => tl!("Orphaned timeline").to_owned(),
        "unreachableMenu" => tl!("Orphaned menu").to_owned(),
        "firstPlayNotSet" => tl!("First Play is not set.").to_owned(),
        "menuWithoutButtons" => tl!("The menu has no buttons.").to_owned(),
        "buttonsOverlap" => tl!("Buttons overlap.").to_owned(),
        "buttonOutOfFrame" => tl!("The button lies outside the frame.").to_owned(),
        "badDefaultButton" => tl!("The default button does not exist.").to_owned(),
        "tooManyButtons" => tl!("There are too many buttons.").to_owned(),
        "graphicOutOfFrame" => tl!("A layer lies outside the frame.").to_owned(),
        "emptySlideshow" => tl!("The slideshow has no slides.").to_owned(),
        "emptyPlaylist" => tl!("The playlist has no items.").to_owned(),
        "emptyChapterPlaylist" => tl!("The chapter playlist has no entries.").to_owned(),
        "assetOffline" => tl!("The file is offline.").to_owned(),
        "undecodableVideo" => {
            tl!("The picture cannot be decoded, so the video cannot be converted for this disc.")
                .to_owned()
        }
        "tooManyChapters" => tl!("There are more than 99 chapters.").to_owned(),
        "tooManyAudio" => tl!("There are more than 8 audio tracks.").to_owned(),
        "tooManySubtitles" => tl!("There are more than 32 subtitle tracks.").to_owned(),
        "firstChapter" => tl!("The first chapter is not at the start.").to_owned(),
        "chapterOrder" => tl!("The chapters are out of order.").to_owned(),
        "timelineBitrateTooHigh" => tl!("The timeline's bit rate is too high.").to_owned(),
        "subtitleTextOverflow" => overflow_text(&problem.message),
        "subtitleSpacing" => spacing_text(&problem.message),
        "playbackType" => {
            tl!("On a DVD, random and shuffle play pick from the first 64 entries only.").to_owned()
        }
        _ => problem.message.clone(),
    }
}

/// Encore's wording of a subtitle that overflows its text frame: the subtitle track with the in and out points
/// (字幕のオーバーフロー: 字幕トラック 1、00;00;04;00 から 00;00;06;01 まで), or the slide of a slideshow.
fn overflow_text(message: &str) -> String {
    let place = message
        .split_once("subtitle overflow: ")
        .map(|(_, place)| place);
    let track = place.and_then(|place| {
        let (track, span) = place.strip_prefix("subtitle track ")?.split_once(", ")?;
        let (start, end) = span.split_once(" to ")?;
        Some(tlf!(
            "Subtitle overflow: subtitle track {n}, {a} to {b}",
            n = track,
            a = start,
            b = end
        ))
    });
    let slide = place.and_then(|place| {
        let slide = place.strip_prefix("slide ")?;
        Some(tlf!("Subtitle overflow: slide {n}", n = slide))
    });
    track
        .or(slide)
        .unwrap_or_else(|| tl!("Subtitle text does not fit.").to_owned())
}

/// The wording of two subtitles that are too close on a Blu-ray disc: the track and where the gap is.
fn spacing_text(message: &str) -> String {
    message
        .split_once(": subtitle track ")
        .and_then(|(_, place)| {
            let (track, span) = place.split_once(", ")?;
            let (start, end) = span.split_once(" to ")?;
            Some(tlf!(
                "Subtitles are less than 5 frames apart: subtitle track {n}, {a} to {b}",
                n = track,
                a = start,
                b = end
            ))
        })
        .unwrap_or_else(|| tl!("Subtitles are less than 5 frames apart.").to_owned())
}

/// The results by item, in the order the items first appear.
#[must_use]
pub(crate) fn check_rows(
    project: &disccraft_project::Project,
    visible: &[&ProblemView],
) -> Vec<CheckRow> {
    let mut rows: Vec<CheckRow> = Vec::new();
    for problem in visible {
        let name =
            subject_name(project, &problem.message).unwrap_or_else(|| tl!("Disc").to_owned());
        let entry = (problem.severity.clone(), problem_text(problem));
        match rows.iter_mut().find(|row| row.name == name) {
            Some(row) => row.problems.push(entry),
            None => rows.push(CheckRow {
                name,
                problems: vec![entry],
            }),
        }
    }
    rows
}

/// Tests for the check window.
#[cfg(test)]
mod tests {
    use super::{CheckCategory, CheckWindow, overflow_text, severity_label, visible_problems};
    use crate::build_panel::ProblemView;
    use crate::i18n::{Language, set_current};

    /// The overflow of a timeline's subtitle names its track and times; a slide's names the slide (whatever the
    /// language of the words around them).
    #[test]
    fn overflow_names_the_subtitle() {
        let text = overflow_text(
            "timeline 3 subtitle overflow: subtitle track 1, 00;00;04;00 to 00;00;06;01",
        );
        assert!(
            text.contains(" 1") && text.contains("00;00;04;00") && text.contains("00;00;06;01"),
            "{text}"
        );
        assert!(!text.contains("timeline 3"), "{text}");
        let text = overflow_text("slideshow 9 subtitle overflow: slide 2");
        assert!(text.contains(" 2") && !text.contains('9'), "{text}");
        assert!(!overflow_text("something else").contains("something"));
    }

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

    /// Encore lists one line per item, and words the common problems its own way.
    #[test]
    fn results_are_grouped_by_item() -> Result<(), Box<dyn std::error::Error>> {
        set_current(Language::En);
        let mut project = disccraft_project::Project::default();
        let video = project.add_asset(std::path::Path::new("a.m2v"))?;
        let timeline = project.add_timeline(video, None)?;
        let name = project
            .timeline(timeline)
            .map(|timeline| timeline.name.clone())
            .unwrap_or_default();
        let problem = |code: &str, message: String| ProblemView {
            severity: "warning".to_owned(),
            code: code.to_owned(),
            message,
        };
        let problems = [
            problem(
                "endActionNotSet",
                format!("timeline {} end action is not set", timeline.0),
            ),
            problem(
                "capacityExceeded",
                "estimated 9 bytes exceeds capacity 1 bytes".to_owned(),
            ),
            problem(
                "unreachableTimeline",
                format!("timeline {} is unreachable", timeline.0),
            ),
        ];
        let visible: Vec<&ProblemView> = problems.iter().collect();
        let rows = super::check_rows(&project, &visible);
        assert_eq!(rows.len(), 2);
        let first = rows.first().ok_or("no rows")?;
        assert_eq!(first.name, name);
        let texts: Vec<&str> = first
            .problems
            .iter()
            .map(|(_, text)| text.as_str())
            .collect();
        assert_eq!(texts, ["The end action is not set.", "Orphaned timeline"]);
        let second = rows.get(1).ok_or("one row")?;
        assert_eq!(second.name, "Disc");
        // A problem with wording of its own, and one that keeps the engine's message.
        let more = [
            problem("menuWithoutButtons", "menu 99 has no buttons".to_owned()),
            problem("navigation", "something else".to_owned()),
        ];
        let visible: Vec<&ProblemView> = more.iter().collect();
        let texts: Vec<String> = super::check_rows(&project, &visible)
            .iter()
            .flat_map(|row| row.problems.iter().map(|(_, text)| text.clone()))
            .collect();
        assert_eq!(texts, ["The menu has no buttons.", "something else"]);
        set_current(Language::Ja);
        Ok(())
    }

    /// The item is read from the head of the engine's message.
    #[test]
    fn the_subject_is_read_from_the_message() {
        assert_eq!(
            super::problem_subject("menu 4 button 2 has no link"),
            Some(("menu", 4, Some(2)))
        );
        assert_eq!(
            super::problem_subject("chapter playlist 7 has no entries"),
            Some(("chapter playlist", 7, None))
        );
        assert_eq!(super::problem_subject("menus mix 4:3 and 16:9"), None);
        assert_eq!(super::problem_subject("timeline x"), None);
    }
}
