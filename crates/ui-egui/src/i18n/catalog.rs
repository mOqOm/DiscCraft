//! Translation catalogs (`*.tsv`).
//!
//! Ported from FilmCraft's `crates/ui-egui/src/i18n/catalog.rs`
//! (https://github.com/storytold/filmcraft, rev `da535c18433dbe046553733b0ab08ce9bd736a51`,
//! MIT OR Apache-2.0). See `NOTICE`.

use std::collections::HashMap;

/// A parsed catalog. Built once per language and kept for the life of the process.
#[derive(Debug, Default)]
pub struct Catalog {
    /// context-free strings: English source → translation (looked up every frame)
    plain: HashMap<String, String>,
    /// strings with a disambiguating context, keyed `context \u{1} source`
    contextual: HashMap<String, String>,
}

/// A catalog entry as read from the file: (context, source, translation).
pub type Entry = (String, String, String);

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();

    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }

        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }

    out
}

/// Read the entries of a catalog file. Malformed lines are returned as errors (and skipped), so a
/// bad translation never breaks the UI; the tests insist the bundled catalogs have none.
pub fn parse_entries(text: &str) -> (Vec<Entry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut errors = Vec::new();

    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }

        let mut cols = line.split('\t');

        match (cols.next(), cols.next(), cols.next(), cols.next()) {
            (Some(ctx), Some(src), Some(tr), None) if !src.is_empty() && !tr.is_empty() => {
                entries.push((unescape(ctx), unescape(src), unescape(tr)));
            }
            _ => errors.push(format!(
                "line {}: expected `context<TAB>source<TAB>translation`",
                n + 1
            )),
        }
    }

    (entries, errors)
}

impl Catalog {
    pub fn parse(text: &str) -> Catalog {
        let mut catalog = Catalog::default();

        for (ctx, src, tr) in parse_entries(text).0 {
            if ctx.is_empty() {
                catalog.plain.insert(src, tr);
            } else {
                catalog.contextual.insert(format!("{ctx}\u{1}{src}"), tr);
            }
        }

        catalog
    }

    pub fn plain(&self, text: &str) -> Option<&str> {
        self.plain.get(text).map(String::as_str)
    }

    pub fn contextual(&self, ctx: &str, text: &str) -> Option<&str> {
        self.contextual
            .get(&format!("{ctx}\u{1}{text}"))
            .map(String::as_str)
    }

    /// The number of context-free entries (used by the report).
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.plain.len()
    }
}

/// `{name}` placeholders of a template, in order of appearance.
#[cfg(test)]
pub fn placeholders(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;

    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];

        match after.find('}') {
            Some(end) => {
                out.push(&after[..end]);
                rest = &after[end + 1..];
            }
            None => break,
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{Catalog, parse_entries, placeholders};

    #[test]
    fn parses_plain_and_contextual_rows() {
        let text = "# comment\n\tBuild\tビルド\nmenu\tBuild\tビルドメニュー\n";
        let (entries, errors) = parse_entries(text);

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(entries.len(), 2);

        let catalog = Catalog::parse(text);
        assert_eq!(catalog.plain("Build"), Some("ビルド"));
        assert_eq!(catalog.contextual("menu", "Build"), Some("ビルドメニュー"));
        assert_eq!(catalog.plain("Missing"), None);
        assert_eq!(catalog.len(), 1);
    }

    #[test]
    fn malformed_rows_are_reported_and_skipped() {
        let text = "\tBuild\n\tBuild\tビルド\textra\n\tBuild\tビルド\n";
        let (entries, errors) = parse_entries(text);

        assert_eq!(entries.len(), 1);
        assert_eq!(errors.len(), 2, "{errors:?}");
    }

    #[test]
    fn escapes_are_resolved() -> Result<(), String> {
        let text = "\tA\\tB\\nC\tx\\ny\n";
        let (entries, errors) = parse_entries(text);

        assert!(errors.is_empty(), "{errors:?}");
        let (ctx, src, tr) = entries.first().ok_or("missing entry")?;

        assert_eq!(ctx, "");
        assert_eq!(src, "A\tB\nC");
        assert_eq!(tr, "x\ny");
        Ok(())
    }

    #[test]
    fn placeholder_order_is_kept() {
        assert_eq!(placeholders("{b} {a} {b}"), vec!["b", "a", "b"]);
        assert_eq!(placeholders("no placeholders"), Vec::<&str>::new());
        assert_eq!(placeholders("broken {a"), Vec::<&str>::new());
    }
}
