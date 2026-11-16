//! Interface translations. Strings in code stay English and are the lookup keys; one catalog per
//! language (`i18n/<code>.tsv`) maps them to display text at draw time. Untranslated strings are
//! shown in English, so coverage can grow incrementally. Command ids, automation ids, the control
//! channel, the CLI and MCP stay English.
//!
//! Ported from FilmCraft's `crates/ui-egui/src/i18n.rs`
//! (https://github.com/storytold/filmcraft, rev `da535c18433dbe046553733b0ab08ce9bd736a51`, unchanged up to `19d46c07`,
//! MIT OR Apache-2.0). See `NOTICE` and [docs/architecture.md](../../../docs/architecture.md) §5.3.
//!
//! # Looking strings up
//! - [`tl`] with a string literal: that UI string in the language the UI is drawn in
//!   (see [`set_current`]). Use it for every label, button, heading, tooltip and hint.
//!   `cargo xtask i18n-report` lists the literals each catalog lacks.
//! - [`t`]: the same for a string that is not a literal.
//! - [`tr_ctx`]: when one English word needs different translations (the Menu panel, the Menu tab).
//! - [`fmt`]: fill `{name}` placeholders after translating a template; translators may reorder them.

mod catalog;

use catalog::Catalog;
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::sync::OnceLock;

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

static JAPANESE: OnceLock<Catalog> = OnceLock::new();

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

    /// The language's catalog (none for English, the source language).
    fn catalog(self) -> Option<&'static Catalog> {
        match self {
            Self::En => None,
            Self::Ja => Some(JAPANESE.get_or_init(|| Catalog::parse(include_str!("i18n/ja.tsv")))),
        }
    }

    /// Translate an English UI string; unknown strings come back unchanged.
    pub(crate) fn tr(self, text: &str) -> &str {
        self.catalog()
            .and_then(|catalog| catalog.plain(text))
            .unwrap_or(text)
    }

    /// Does this language's catalog translate `text`? (English never does: it is the source.)
    #[cfg(test)]
    pub(crate) fn has(self, text: &str) -> bool {
        self.catalog()
            .and_then(|catalog| catalog.plain(text))
            .is_some()
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

/// Like [`t`], for an English string that needs a disambiguating `context`; falls back to the
/// context-free translation.
#[allow(dead_code)]
pub(crate) fn tr_ctx<'a>(context: &str, text: &'a str) -> &'a str {
    let language = current();

    language
        .catalog()
        .and_then(|catalog| catalog.contextual(context, text))
        .unwrap_or_else(|| language.tr(text))
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
    use super::{Language, catalog::parse_entries, current, fmt, literals_in, set_current, t};
    use std::error::Error;

    /// The bundled Japanese catalog must parse without a single malformed row.
    #[test]
    fn japanese_catalog_is_well_formed() -> Result<(), Box<dyn Error>> {
        let (_entries, errors) = parse_entries(include_str!("i18n/ja.tsv"));

        if errors.is_empty() {
            return Ok(());
        }

        Err(errors.join("\n").into())
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
