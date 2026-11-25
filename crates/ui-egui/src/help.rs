//! ヘルプメニュー（FilmCraft のヘルプメニューと同じ並び）: ArtCraft の Discord に参加… / ArtCraft の Web サイト /
//! GitHub の DiscCraft / 問題を報告… / ログファイルを表示… / システム互換性レポート… / アップデートを確認… /
//! DiscCraft について / DiscCraft ヘルプ…（F1）。
//!
//! FilmCraft の「getartcraft.com の FilmCraft」にあたる項目はない（getartcraft.com に DiscCraft のページがない）。
//! 「アップデートを確認…」は、このビルドの版を示して GitHub のリリースを開く（FilmCraft のように自分で確かめて
//! 入れ替えることはしない: DiscCraft は通信の部品を持たず、リリースはインストーラーではなく exe そのもの）。

use crate::actions::Action;
use crate::app::DiscCraftApp;
use crate::i18n::{Language, tl, tlf};

use egui::{Context, Id, Modal, Ui};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once, PoisonError};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The ArtCraft community Discord (the same as FilmCraft's Help menu).
pub(crate) const DISCORD: &str = "https://discord.gg/artcraft";
/// The ArtCraft website.
pub(crate) const WEBSITE: &str = "https://getartcraft.com";
/// DiscCraft's public repository.
pub(crate) const GITHUB: &str = "https://github.com/mOqOm/DiscCraft";
/// Where problems are reported.
pub(crate) const ISSUES: &str = "https://github.com/mOqOm/DiscCraft/issues";
/// The newest release.
pub(crate) const RELEASES: &str = "https://github.com/mOqOm/DiscCraft/releases/latest";

/// The user manual in the interface's language.
pub(crate) fn manual_url(language: Language) -> &'static str {
    match language {
        Language::Ja => "https://github.com/mOqOm/DiscCraft/blob/main/docs/user-guide.ja.md",
        Language::En => "https://github.com/mOqOm/DiscCraft/blob/main/docs/user-guide.md",
    }
}

/// The release this build was made for: `DISCCRAFT_RELEASE` when the published executables are built (for
/// example `v0.1.0-alpha.5`); `None` for a build from the source.
pub(crate) fn release_tag() -> Option<&'static str> {
    option_env!("DISCCRAFT_RELEASE").filter(|tag| !tag.trim().is_empty())
}

/// The version as people see it: the package version and the release, or that it is a build from the source.
pub(crate) fn version_text() -> String {
    match release_tag() {
        Some(tag) => format!("{} ({tag})", env!("CARGO_PKG_VERSION")),
        None => format!(
            "{} ({})",
            env!("CARGO_PKG_VERSION"),
            tl!("development build")
        ),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Session log (Help ▸ Reveal Log Files…)
// ---------------------------------------------------------------------------------------------------------------

/// The most lines a session keeps (the oldest tenth goes when it is full).
const MAX_LOG_LINES: usize = 5000;
/// How many session logs are kept in the log folder.
const KEEP_LOG_FILES: usize = 20;

/// What happened in this session: the commands run, their errors and the status messages.
struct SessionLog {
    started: Instant,
    started_at: SystemTime,
    lines: Vec<String>,
}

static LOG: Mutex<Option<SessionLog>> = Mutex::new(None);

/// Adds a line to the session log (one line however many lines `text` has).
pub(crate) fn log(text: &str) {
    let mut guard = LOG.lock().unwrap_or_else(PoisonError::into_inner);
    let log = guard.get_or_insert_with(|| SessionLog {
        started: Instant::now(),
        started_at: SystemTime::now(),
        lines: Vec::new(),
    });
    if log.lines.len() >= MAX_LOG_LINES {
        let _ = log.lines.drain(..MAX_LOG_LINES / 10);
    }
    let seconds = log.started.elapsed().as_secs_f64();
    let text = text.replace(['\r', '\n'], " ");
    log.lines.push(format!("[{seconds:10.3}] {text}"));
}

/// The folder session logs are written to.
pub(crate) fn logs_dir() -> Option<PathBuf> {
    crate::workspace::config_dir().map(|dir| dir.join("disccraft").join("logs"))
}

/// The session log as a file's text, and the time stamp of its file name.
fn session_text() -> (String, String) {
    let guard = LOG.lock().unwrap_or_else(PoisonError::into_inner);
    let started_at = guard
        .as_ref()
        .map_or_else(SystemTime::now, |log| log.started_at);
    let (date, stamp) = utc_stamps(started_at);
    let mut text = format!(
        "DiscCraft {}\nStarted: {date} UTC\nSystem: {} ({})\n\n",
        version_text(),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    for line in guard.iter().flat_map(|log| log.lines.iter()) {
        text.push_str(line);
        text.push('\n');
    }
    (text, stamp)
}

/// Writes this session's log to the log folder (`disccraft-<started, UTC>.log`, the older ones beyond
/// [`KEEP_LOG_FILES`] removed) and returns its path.
pub(crate) fn write_session_log() -> Result<PathBuf, String> {
    let dir = logs_dir().ok_or_else(|| tl!("Cannot find where to save settings").to_owned())?;
    std::fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let (text, stamp) = session_text();
    let path = dir.join(format!("disccraft-{stamp}.log"));
    std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    prune_logs(&dir);
    Ok(path)
}

/// Removes the oldest session logs beyond [`KEEP_LOG_FILES`] (their names sort by when they started).
fn prune_logs(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("disccraft-") && name.ends_with(".log"))
        })
        .collect();
    logs.sort();
    let extra = logs.len().saturating_sub(KEEP_LOG_FILES);
    for old in logs.into_iter().take(extra) {
        let _ = std::fs::remove_file(old);
    }
}

/// Keeps what media files were found to be (length, picture size, thumbnails) between runs, in a folder beside
/// the settings, so a project with long videos opens without reading them again. Called once by the application
/// at start (not by tests, which leave the user's folders alone).
pub fn enable_media_cache() {
    if let Some(dir) = crate::workspace::media_cache_dir() {
        disccraft_engine::enable_media_cache(&dir);
    }
}

/// Writes the session log when the program panics, so a crash leaves a log behind (the panic itself is reported
/// as before). Called once by the application at start.
pub fn install_crash_log() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log(&format!("panic: {info}"));
            let _ = write_session_log();
            previous(info);
        }));
    });
}

/// `time` in UTC as `YYYY-MM-DD hh:mm:ss` and as `YYYYMMDD-hhmmss` (for a file name).
fn utc_stamps(time: SystemTime) -> (String, String) {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let seconds = i64::try_from(seconds).unwrap_or(0);
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let rest = seconds.rem_euclid(86_400);
    let (hour, minute, second) = (rest / 3600, rest % 3600 / 60, rest % 60);
    (
        format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}"),
        format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}"),
    )
}

/// The calendar date of a day counted from 1970-01-01 (the proleptic Gregorian calendar; the usual conversion
/// through 400-year eras of 146 097 days).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

// ---------------------------------------------------------------------------------------------------------------
// System Compatibility Report
// ---------------------------------------------------------------------------------------------------------------

/// The report: (label, value) rows, in order.
pub(crate) fn system_report(language: Language) -> Vec<(&'static str, String)> {
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let mut cpu = tlf!("{n} hardware threads", n = threads);
    if threads < 4 {
        cpu.push_str(tl!(" — 4 or more are recommended for converting video"));
    }
    let folder = |path: Option<PathBuf>| {
        path.map_or_else(|| "-".to_owned(), |path| path.display().to_string())
    };
    let (video, audio, images, subtitles) = readable_files();
    vec![
        (tl!("DiscCraft"), version_text()),
        (
            tl!("Operating system"),
            format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH),
        ),
        (tl!("CPU"), cpu),
        (
            tl!("System language"),
            sys_locale::get_locale().unwrap_or_else(|| "-".to_owned()),
        ),
        (tl!("Display language"), language.name().to_owned()),
        (
            tl!("Settings folder"),
            folder(crate::workspace::config_dir().map(|dir| dir.join("disccraft"))),
        ),
        (tl!("Log folder"), folder(logs_dir())),
        (
            tl!("Cache of Craft documents"),
            std::env::temp_dir()
                .join("disccraft-sessions")
                .display()
                .to_string(),
        ),
        (tl!("Video"), video),
        (tl!("Audio"), audio),
        (tl!("Images"), images),
        (tl!("Subtitles"), subtitles),
        (
            tl!("DVD output"),
            tl!("VIDEO_TS folder / ISO: MPEG-2, AC-3, LPCM, DTS (as it is), subpictures").to_owned(),
        ),
        (
            tl!("Blu-ray output"),
            tl!("BDMV folder / ISO: MPEG-2 (SD), H.264 1080p / 720p, AC-3, LPCM, PG subtitles, IG menus")
                .to_owned(),
        ),
    ]
}

/// The file types DiscCraft reads, by kind: video, audio, images and subtitles.
fn readable_files() -> (String, String, String, String) {
    let mut kinds: [Vec<&str>; 4] = Default::default();
    for extension in crate::file_drop::MEDIA_EXTENSIONS {
        let kind =
            disccraft_project::AssetKind::guess_from_path(Path::new(&format!("file.{extension}")));
        let slot = match kind {
            disccraft_project::AssetKind::Video => kinds.get_mut(0),
            disccraft_project::AssetKind::Audio => kinds.get_mut(1),
            disccraft_project::AssetKind::Image => kinds.get_mut(2),
            disccraft_project::AssetKind::Subtitle => kinds.get_mut(3),
            _ => None,
        };
        if let Some(slot) = slot {
            slot.push(extension);
        }
    }
    let [video, audio, images, subtitles] = kinds.map(|list| list.join(", "));
    (video, audio, images, subtitles)
}

/// The report as text, for the clipboard (one `label: value` line a row).
pub(crate) fn report_text(rows: &[(&str, String)]) -> String {
    rows.iter()
        .map(|(label, value)| format!("{label}: {value}\n"))
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------
// The menu and its dialogs
// ---------------------------------------------------------------------------------------------------------------

/// The Help menu's items, in FilmCraft's order.
pub(crate) const HELP_ITEMS: [Action; 9] = [
    Action::JoinDiscord,
    Action::ArtCraftWebsite,
    Action::DiscCraftOnGitHub,
    Action::ReportIssue,
    Action::RevealLogFiles,
    Action::SystemReport,
    Action::CheckForUpdates,
    Action::About,
    Action::Help,
];

/// What a Help menu item does, shown while the pointer rests on it: the address a link opens, or what an item
/// that stays inside DiscCraft does.
pub(crate) fn menu_hint(action: Action, language: Language) -> String {
    match action {
        Action::JoinDiscord => DISCORD.to_owned(),
        Action::ArtCraftWebsite => WEBSITE.to_owned(),
        Action::DiscCraftOnGitHub => GITHUB.to_owned(),
        Action::ReportIssue => ISSUES.to_owned(),
        Action::Help => manual_url(language).to_owned(),
        Action::RevealLogFiles => {
            tl!("Saves this session's log and shows it in the file manager.").to_owned()
        }
        Action::SystemReport => tl!("Shows a report on this computer.").to_owned(),
        Action::CheckForUpdates => tl!("Shows this version and the releases on GitHub.").to_owned(),
        Action::About => tl!("Shows the version, community links and license.").to_owned(),
        _ => String::new(),
    }
}

impl DiscCraftApp {
    /// ヘルプメニュー。項目にポインターを置くと、開くアドレスか、することが出る。
    pub(crate) fn help_menu(&self, ui: &mut Ui, clicked: &mut Vec<Action>) {
        for action in HELP_ITEMS {
            let button =
                egui::Button::new(action.label()).shortcut_text(action.shortcut_text(ui.ctx()));
            if ui
                .add(button)
                .on_hover_text(menu_hint(action, self.language))
                .clicked()
            {
                clicked.push(action);
            }
        }
    }

    /// Opens `url` in the web browser and says so in the status bar.
    pub(crate) fn open_link(&mut self, ctx: &Context, url: &str) {
        ctx.open_url(egui::OpenUrl::new_tab(url));
        self.status = tlf!("Opened {url}", url = url);
    }

    /// ヘルプ ▸ ログファイルを表示…: このセッションのログを書き出し、ファイルマネージャーで見せる。
    pub(crate) fn reveal_log_files(&mut self) {
        match write_session_log() {
            Ok(path) => {
                self.reveal_in_file_manager(&path);
                self.status = tlf!("Saved the log: {path}", path = path.display());
            }
            Err(error) => self.status = error,
        }
    }

    /// ヘルプ ▸ システム互換性レポート…（開いているときだけ描く）。
    pub(crate) fn system_report_window(&mut self, ctx: &Context) {
        if !self.system_report_open {
            return;
        }
        let rows = system_report(self.language);
        let mut close = false;
        let response = Modal::new(Id::unique("system_report")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.heading(tl!("System Compatibility Report"));
            ui.add_space(6.0);
            egui::Grid::new("system_report_rows")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    for (label, value) in &rows {
                        let _ = ui.label(*label);
                        let _ = ui.add(egui::Label::new(value.as_str()).wrap());
                        ui.end_row();
                    }
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add(crate::theme::dialog_button(tl!("Copy")))
                    .on_hover_text(tl!("Copies the report, to paste into a problem report."))
                    .clicked()
                {
                    ui.ctx().copy_text(report_text(&rows));
                }
                if ui.add(crate::theme::default_button(tl!("Close"))).clicked() {
                    close = true;
                }
            });
        });
        if close || response.should_close() {
            self.system_report_open = false;
        }
    }

    /// ヘルプ ▸ アップデートを確認…（開いているときだけ描く）。
    pub(crate) fn update_window(&mut self, ctx: &Context) {
        if !self.update_open {
            return;
        }
        let mut close = false;
        let mut open_releases = false;
        let response = Modal::new(Id::unique("check_for_updates")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading(tl!("Check for Updates"));
            ui.add_space(6.0);
            let _ = ui.label(tlf!("This version: {version}", version = version_text()));
            ui.add_space(4.0);
            let _ = ui.label(tl!(
                "DiscCraft does not look for updates by itself. New releases are published on GitHub."
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                open_releases = ui
                    .add(crate::theme::dialog_button(tl!("Open the Releases on GitHub")))
                    .on_hover_text(RELEASES)
                    .clicked();
                if ui.add(crate::theme::default_button(tl!("Close"))).clicked() {
                    close = true;
                }
            });
        });
        if open_releases {
            self.open_link(ctx, RELEASES);
        }
        if close || response.should_close() {
            self.update_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HELP_ITEMS, civil_from_days, manual_url, menu_hint, readable_files, report_text, utc_stamps,
    };
    use crate::actions::Action;
    use crate::i18n::Language;
    use std::time::{Duration, UNIX_EPOCH};

    /// The menu has FilmCraft's items in FilmCraft's order (without the getartcraft.com page DiscCraft does not
    /// have), and the links point at the ArtCraft community and DiscCraft's repository.
    #[test]
    fn the_help_menu_follows_filmcraft() {
        assert_eq!(HELP_ITEMS.first(), Some(&Action::JoinDiscord));
        assert_eq!(HELP_ITEMS.last(), Some(&Action::Help));
        assert_eq!(
            menu_hint(Action::JoinDiscord, Language::En),
            "https://discord.gg/artcraft"
        );
        assert!(menu_hint(Action::ReportIssue, Language::En).ends_with("/DiscCraft/issues"));
        assert!(manual_url(Language::Ja).ends_with("user-guide.ja.md"));
        assert!(manual_url(Language::En).ends_with("user-guide.md"));
        assert_eq!(
            menu_hint(Action::Help, Language::Ja),
            manual_url(Language::Ja)
        );
        for action in HELP_ITEMS {
            assert!(!menu_hint(action, Language::En).is_empty(), "{action:?}");
            assert!(!action.label().is_empty());
        }
    }

    /// Dates come out right across leap years and the turn of a century.
    #[test]
    fn days_become_calendar_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(59), (1970, 3, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_773), (2026, 11, 16));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        let time = UNIX_EPOCH + Duration::from_secs(20_773 * 86_400 + 13 * 3600 + 5 * 60 + 9);
        assert_eq!(
            utc_stamps(time),
            (
                "2026-11-16 13:05:09".to_owned(),
                "20261116-130509".to_owned()
            )
        );
    }

    /// The report lists the file types by kind, and its text has one line a row.
    #[test]
    fn the_report_lists_readable_files_by_kind() {
        let (video, audio, images, subtitles) = readable_files();
        assert!(video.contains("mp4") && video.contains("vob") && !video.contains("wav"));
        assert!(audio.contains("wav") && audio.contains("ac3") && audio.contains("opus"));
        assert!(images.contains("png") && images.contains("psd"));
        assert!(subtitles.contains("srt") && subtitles.contains("sup"));
        let text = report_text(&[("A", "1".to_owned()), ("B", "2".to_owned())]);
        assert_eq!(text, "A: 1\nB: 2\n");
    }
}
