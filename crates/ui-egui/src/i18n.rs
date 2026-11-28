//! Interface translations. Strings in code stay English and are the lookup keys; the catalogs map them to display
//! text at draw time. Untranslated strings are shown in English. Command ids, automation ids, the control channel, the
//! CLI and MCP stay English.
//!
//! The catalogs are in craft-i18n's layout (storytold/craft-i18n), looked up with craft-libs' `craft-i18n` crate as
//! the other Crafting Apps do (M16.1): for Japanese, DiscCraft's own rows (`i18n/locales/ja/disccraft.tsv`) over the
//! strings the apps share (`i18n/locales/ja/common.tsv`, a copy of craft-i18n's shared catalog at the commit in
//! `i18n/craft-i18n.rev`), then the English. So a string DiscCraft shares with the other apps reads as it does there.
//!
//! The macros follow FilmCraft's `crates/ui-egui/src/i18n.rs`
//! (https://github.com/storytold/filmcraft, rev `da535c18433dbe046553733b0ab08ce9bd736a51`, unchanged up to `19d46c07`,
//! MIT OR Apache-2.0). See `NOTICE` and [docs/architecture.md](../../../docs/architecture.md) §5.3.
//!
//! # Looking strings up
//! - [`tl`] with a string literal: that UI string in the language the UI is drawn in
//!   (see [`set_current`]). Use it for every label, button, heading, tooltip and hint.
//! - [`t`]: the same for a string that is not a literal.
//! - [`tr_ctx`]: when one English word needs different translations (the Menu panel, the Menu tab).
//! - [`fmt`]: fill `{name}` placeholders after translating a template; translators may reorder them.

use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::sync::LazyLock;

/// The catalogs, one (locale, shared catalog, DiscCraft's catalog) entry per language, as craft-i18n's `Embedded`.
static CATALOGS: &craft_i18n::Embedded = &[(
    "ja",
    include_str!("../i18n/locales/ja/common.tsv"),
    include_str!("../i18n/locales/ja/disccraft.tsv"),
)];

/// The languages the interface offers besides English (the codes of [`Language::ALL`]).
const OFFERED: [&str; 1] = ["ja"];

/// The catalogs, ready for lookups. Each language is parsed on its first lookup; a bad row is skipped (the tests
/// insist there is none).
static I18N: LazyLock<std::sync::Arc<craft_i18n::I18n>> =
    LazyLock::new(|| std::sync::Arc::new(craft_i18n::I18n::new(CATALOGS, &OFFERED).0));

/// The catalogs and `language` in craft-i18n's terms, for the UI runtime's host (M9.54): the framework's own words
/// (the dock's, the menu bar's) are looked up in the same catalogs as DiscCraft's.
pub(crate) fn shared(language: Language) -> (std::sync::Arc<craft_i18n::I18n>, craft_i18n::Lang) {
    (std::sync::Arc::clone(&I18N), language.lang())
}

/// Translate a UI string literal into the language the interface is drawn in.
macro_rules! tl {
    ($s:literal) => {
        $crate::i18n::t($s)
    };
}

/// A translated template with `{name}` placeholders filled in. After the template literal come the
/// values: `n = count`, or just `count` for a variable of the same name.
macro_rules! tlf {
    (@v $k:ident = $v:expr) => {
        $v
    };
    (@v $k:ident) => {
        $k
    };
    ($s:literal $(, $k:ident $(= $v:expr)?)* $(,)?) => {
        $crate::i18n::fmt(
            $crate::i18n::t($s),
            &[$((stringify!($k), ToString::to_string(&tlf!(@v $k $(= $v)?)).as_str())),*],
        )
    };
}

pub(crate) use tl;
pub(crate) use tlf;

/// The interface language. English is the source language, so it has no catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Language {
    /// English, the language the strings in code are written in.
    En,
    /// Japanese. The default, because the project starts from a Japanese interface.
    #[default]
    Ja,
}

impl Language {
    /// Every language the interface offers.
    pub(crate) const ALL: [Self; 2] = [Self::En, Self::Ja];

    /// The language's own name, shown in the language menu.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Ja => "日本語",
        }
    }

    /// Stable code (ISO 639-1), the value saved in the workspace file and the language of audio without its own.
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ja => "ja",
        }
    }

    #[cfg(test)]
    pub(crate) fn parse(code: &str) -> Option<Self> {
        match code {
            "en" => Some(Self::En),
            "ja" => Some(Self::Ja),
            _ => None,
        }
    }

    /// The language in craft-i18n's terms (English, the source language, has no catalog).
    fn lang(self) -> craft_i18n::Lang {
        match self {
            Self::En => craft_i18n::Lang::EN,
            Self::Ja => I18N.from_code(self.code()).unwrap_or(craft_i18n::Lang::EN),
        }
    }

    /// Translate an English UI string; unknown strings come back unchanged.
    pub(crate) fn tr(self, text: &str) -> &str {
        I18N.t(self.lang(), text)
    }

    /// Does this language translate `text`, in DiscCraft's catalog or the shared one? (English never does: it is the
    /// source.)
    #[cfg(test)]
    pub(crate) fn has(self, text: &str) -> bool {
        self != Self::En && I18N.get(self.lang(), "", text).is_some()
    }

    /// The interface language for an operating system whose display language is `tag` (a BCP 47 or POSIX locale
    /// tag such as `ja-JP` or `ja_JP.UTF-8`): Japanese for Japanese, English for English and for every language the
    /// interface does not have.
    pub(crate) fn from_locale(tag: &str) -> Self {
        let primary = tag.split(['-', '_', '.', '@']).next().unwrap_or_default();
        if primary.eq_ignore_ascii_case("ja") {
            Self::Ja
        } else {
            Self::En
        }
    }
}

/// The interface language to start with when the user has not chosen one: the operating system's display language
/// (Japanese on a Japanese system, English on an English one), and English on a system in any other language or
/// when the system does not say.
///
/// The display language comes from the operating system (`sys-locale`: the user's preferred UI language on
/// Windows, the locale on macOS and Linux), else from the usual environment variables.
pub(crate) fn detected() -> Language {
    let system = sys_locale::get_locale().or_else(|| {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(|value| value.to_string_lossy().into_owned())
            .find(|value| !value.is_empty())
    });
    system.map_or(Language::En, |tag| Language::from_locale(&tag))
}

thread_local! {
    /// The language the UI is drawn in. Per thread, so tests that switch language don't leak it
    /// into tests running on other threads.
    static CURRENT: Cell<Language> = const { Cell::new(Language::Ja) };
}

/// Set the language for drawing. Called at the start of every frame from the UI state.
pub(crate) fn set_current(language: Language) {
    CURRENT.with(|current| current.set(language));
}

/// The language the UI is drawn in.
pub(crate) fn current() -> Language {
    CURRENT.with(Cell::get)
}

/// Translate an English UI string into the current language.
pub(crate) fn t(text: &str) -> &str {
    current().tr(text)
}

/// Whether the Japanese catalog has an entry for `text` under `context` (M6.397: the Metadata panel's names).
#[cfg(test)]
pub(crate) fn has_contextual(context: &str, text: &str) -> bool {
    I18N.get(Language::Ja.lang(), context, text).is_some()
}

/// Like [`t`], for an English string that needs a disambiguating `context`; falls back to the
/// context-free translation.
#[allow(dead_code)]
pub(crate) fn tr_ctx<'a>(context: &str, text: &'a str) -> &'a str {
    I18N.tc(current().lang(), context, text)
}

/// Fill `{name}` placeholders. Unknown placeholders are left as written.
pub(crate) fn fmt(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::new();
    let mut rest = template;

    while let Some((prefix, after)) = rest.split_once('{') {
        out.push_str(prefix);

        let Some((name, tail)) = after.split_once('}') else {
            out.push('{');
            out.push_str(after);
            return out;
        };

        if let Some((_, value)) = args.iter().find(|(key, _)| *key == name) {
            out.push_str(value);
        } else {
            out.push('{');
            out.push_str(name);
            out.push('}');
        }

        rest = tail;
    }

    out.push_str(rest);
    out
}

/// Every `tl!("…")` / `tlf!("…", …)` literal in a source file (used by the coverage test).
#[cfg(test)]
pub(crate) fn literals_in(source: &str) -> Vec<String> {
    let mut found = Vec::new();

    // Match `tl!(` and `tlf!(`, then an optional run of whitespace (a template may sit on the next line), then the literal.
    for marker in ["tl!(", "tlf!("] {
        let mut rest = source;
        while let Some(position) = rest.find(marker) {
            let after = rest[position + marker.len()..].trim_start();
            rest = after;

            let Some(body) = after.strip_prefix('"') else {
                continue;
            };
            let Some(end) = body.find('"') else {
                break;
            };

            found.push(body[..end].to_string());
            rest = &body[end + 1..];
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use super::{CATALOGS, I18N, Language, current, fmt, literals_in, set_current, t};
    use std::collections::HashMap;
    use std::error::Error;

    /// The bundled catalogs must parse without a single malformed row (as craft-i18n's checker requires).
    #[test]
    fn catalogs_are_well_formed() -> Result<(), Box<dyn Error>> {
        let problems = I18N.check_all();

        if problems.is_empty() {
            return Ok(());
        }

        Err(problems.join("\n").into())
    }

    /// (context, source) → translation of a catalog's rows.
    fn rows(text: &str) -> HashMap<(&str, &str), &str> {
        text.lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| {
                let mut columns = line.split('\t');
                Some(((columns.next()?, columns.next()?), columns.next()?))
            })
            .collect()
    }

    /// craft-i18n's rule for an app catalog: a row that repeats the shared catalog's translation changes nothing, so
    /// it is left out (and a shared string then reads as in the other apps). DiscCraft's overrides of shared strings
    /// are only those it uses with another meaning.
    #[test]
    fn disccraft_rows_override_shared_ones_only_to_change_them() {
        for (_, shared, app) in CATALOGS {
            let shared = rows(shared);
            let app = rows(app);
            let repeated: Vec<_> = app
                .iter()
                .filter(|(key, translation)| shared.get(*key) == Some(*translation))
                .map(|((_, source), _)| *source)
                .collect();
            assert!(
                repeated.is_empty(),
                "rows that repeat common.tsv: {repeated:?}"
            );
            let mut overrides: Vec<_> = app
                .keys()
                .filter(|key| shared.contains_key(*key))
                .map(|(_, source)| *source)
                .collect();
            overrides.sort_unstable();
            assert_eq!(
                overrides,
                ["Deleted", "Enter", "Format:", "Location:", "Number", "Tile"],
                "a new override of a shared string: is the meaning really different?"
            );
        }
    }

    /// Every command's label has a Japanese name: the Edit menu's Undo / Redo and History show them (M16.3: the
    /// labels are English, translated like the rest of the interface).
    #[test]
    fn every_command_label_has_a_japanese_name() {
        let missing: Vec<&str> = disccraft_engine::commands()
            .iter()
            .map(|spec| spec.label)
            .filter(|label| !Language::Ja.has(label))
            .collect();
        assert!(
            missing.is_empty(),
            "command labels without Japanese: {missing:?}"
        );
    }

    /// A shared string reads as craft-i18n's shared catalog has it.
    #[test]
    fn shared_strings_read_as_in_the_other_apps() {
        assert_eq!(Language::Ja.tr("Select All"), "すべて選択");
        assert_eq!(Language::Ja.tr("History"), "履歴");
        // An override: a folder's location, not a position.
        assert_eq!(Language::Ja.tr("Location:"), "場所:");
        assert_eq!(Language::En.tr("Select All"), "Select All");
    }

    /// Every `tl!` literal in the sources must have a Japanese entry, so nothing shows in English
    /// by accident.
    #[test]
    fn every_literal_has_a_japanese_translation() -> Result<(), Box<dyn Error>> {
        let mut missing: Vec<String> = Vec::new();

        // Every source file of the crate (new views are covered without listing them), except this module, whose doc
        // examples are not UI strings.
        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir("src")?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
            .filter(|path| path.file_name().is_some_and(|name| name != "i18n.rs"))
            .collect();
        paths.sort();

        for path in &paths {
            let path = path.display().to_string();
            let Ok(text) = std::fs::read_to_string(&path) else {
                return Err(format!("cannot read {path}").into());
            };

            for literal in literals_in(&text) {
                if !Language::Ja.has(&literal) {
                    missing.push(format!("{path}: \"{literal}\""));
                }
            }
        }

        if missing.is_empty() {
            return Ok(());
        }

        Err(format!("missing Japanese translations:\n{}", missing.join("\n")).into())
    }

    #[test]
    fn untranslated_text_shows_in_english() {
        let before = current();
        set_current(Language::Ja);
        let text = t("A string no catalog knows");
        set_current(before);

        assert_eq!(text, "A string no catalog knows");
    }

    #[test]
    fn switching_languages_changes_the_text() {
        let before = current();

        set_current(Language::En);
        let en = t("Project");

        set_current(Language::Ja);
        let ja = t("Project");

        set_current(before);

        assert_eq!(en, "Project");
        assert_eq!(ja, "プロジェクト");
    }

    #[test]
    fn placeholders_can_be_reordered_by_the_translation() {
        assert_eq!(fmt("{b} then {a}", &[("a", "1"), ("b", "2")]), "2 then 1");
        assert_eq!(fmt("{a}/{b}", &[("a", "x")]), "x/{b}");
        assert_eq!(fmt("plain", &[]), "plain");
        assert_eq!(fmt("broken {a", &[("a", "x")]), "broken {a");
    }

    #[test]
    fn literal_scanner_finds_translated_strings() {
        let source = "ui.label(tl!(\"Build\")); ui.label(tlf!(\"Chapters: {n}\", n = 3));";
        assert_eq!(
            literals_in(source),
            vec!["Build".to_string(), "Chapters: {n}".to_string()]
        );
    }

    #[test]
    fn language_codes_round_trip() {
        for language in Language::ALL {
            assert_eq!(Language::parse(language.code()), Some(language));
        }

        assert_eq!(Language::parse("de"), None);
        assert_eq!(Language::default(), Language::Ja);
    }

    #[test]
    fn the_system_language_picks_japanese_or_english() {
        assert_eq!(Language::from_locale("ja_JP.UTF-8"), Language::Ja);
        assert_eq!(Language::from_locale("ja-JP"), Language::Ja);
        assert_eq!(Language::from_locale("JA"), Language::Ja);
        assert_eq!(Language::from_locale("en-GB"), Language::En);
        assert_eq!(Language::from_locale("en_US.UTF-8"), Language::En);
        // A language the interface does not have, and no language at all, are English.
        assert_eq!(Language::from_locale("de_DE.UTF-8"), Language::En);
        assert_eq!(Language::from_locale("zh-Hans-CN"), Language::En);
        assert_eq!(Language::from_locale("C"), Language::En);
        assert_eq!(Language::from_locale(""), Language::En);
        // Whatever the system says, detection gives a language the interface has.
        assert!(Language::ALL.contains(&super::detected()));
    }
}
