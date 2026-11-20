//! Character formatting of part of a menu text (M6.389): Encore's Character panel on characters selected with the
//! text tool. A [`MenuText`] keeps its own formatting for the whole text and a list of spans, each a run of
//! characters with the fields it sets over the text's.
//!
//! Spans are counted in characters (not bytes) of [`MenuText::text`], kept in order, apart, inside the text and
//! with at least one field set. Every change goes through one character at a time (a text is at most
//! [`crate::MAX_TEXT_CHARS`] characters), which keeps those rules whatever came in.

use serde::{Deserialize, Serialize};

use crate::menu::{MAX_TEXT_SIZE, MIN_TEXT_SIZE, MenuText, TextCaps};

/// The most spans one text holds.
pub const MAX_TEXT_SPANS: usize = 256;

/// The character fields a span sets over its text's own (each `None`: the text's).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SpanStyle {
    /// Font family.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Style of the family ("Bold", "Light", ...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_style: Option<String>,
    /// Size in pixels ([`MIN_TEXT_SIZE`] to [`MAX_TEXT_SIZE`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u16>,
    /// Colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    /// Faux bold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    /// Faux italic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    /// Underline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    /// Strikethrough.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strikethrough: Option<bool>,
    /// All caps / small caps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caps: Option<TextCaps>,
    /// Tracking in 1/1000 em.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracking: Option<i16>,
    /// Baseline shift in pixels, positive up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<i16>,
}

impl SpanStyle {
    /// Whether it sets nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Its values brought into range (a project file can hold anything).
    fn checked(mut self) -> Self {
        self.size = self
            .size
            .map(|size| size.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE));
        self.tracking = self
            .tracking
            .map(|value| value.clamp(crate::MIN_TRACKING, crate::MAX_TRACKING));
        self.baseline = self
            .baseline
            .map(|value| value.clamp(crate::menu::MIN_BASELINE, crate::menu::MAX_BASELINE));
        for name in [&mut self.font, &mut self.font_style] {
            if let Some(found) = name.as_mut() {
                *found = found.chars().take(crate::MAX_FONT_NAME_CHARS).collect();
            }
        }
        self
    }
}

/// A run of a text's characters with formatting of its own.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSpan {
    /// The first character it covers (counted in characters of the text).
    pub start: u32,
    /// The character after its last.
    pub end: u32,
    /// What it sets.
    #[serde(flatten)]
    pub style: SpanStyle,
}

impl MenuText {
    /// The formatting each character of the text has of its own (from its spans; empty where none).
    #[must_use]
    pub fn char_styles(&self) -> Vec<SpanStyle> {
        styles_of(&self.spans, self.text.chars().count())
    }

    /// What character `index` sets over the text's formatting (empty past the end, or where no span is).
    #[must_use]
    pub fn span_style_at(&self, index: usize) -> SpanStyle {
        self.spans
            .iter()
            .find(|span| {
                (span.start as usize..span.end as usize).contains(&index)
                    && index < self.text.chars().count()
            })
            .map(|span| span.style.clone())
            .unwrap_or_default()
    }

    /// Changes the formatting of the characters in `range` (counted in characters, cut to the text) with
    /// `change`.
    pub fn style_chars(&mut self, range: std::ops::Range<usize>, change: &dyn Fn(&mut SpanStyle)) {
        let mut styles = self.char_styles();
        let end = range.end.min(styles.len());
        for style in styles
            .get_mut(range.start.min(end)..end)
            .into_iter()
            .flatten()
        {
            change(style);
        }
        self.spans = spans_of(&styles);
    }

    /// Changes every character's own formatting with `change` (a field set for the whole text takes it out of
    /// the spans: every character then has the text's).
    pub fn style_all_chars(&mut self, change: &dyn Fn(&mut SpanStyle)) {
        let count = self.text.chars().count();
        self.style_chars(0..count, change);
    }

    /// Puts `text` in place of the text, its spans following the edit: the characters before and after the
    /// changed part keep theirs, the characters typed in take those of the character before them (of the first
    /// one when typed at the start).
    pub fn retext(&mut self, text: String) {
        let old: Vec<char> = self.text.chars().collect();
        let new: Vec<char> = text.chars().collect();
        let styles = self.char_styles();
        let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let room = old.len().min(new.len()) - prefix;
        let suffix = old
            .iter()
            .rev()
            .zip(new.iter().rev())
            .take(room)
            .take_while(|(a, b)| a == b)
            .count();
        let typed = new.len() - prefix - suffix;
        let typed_style = prefix
            .checked_sub(1)
            .and_then(|before| styles.get(before))
            .or_else(|| styles.get(prefix))
            .cloned()
            .unwrap_or_default();
        let mut next: Vec<SpanStyle> = Vec::with_capacity(new.len());
        next.extend(styles.iter().take(prefix).cloned());
        next.extend(std::iter::repeat_n(typed_style, typed));
        next.extend(styles.iter().skip(old.len() - suffix).cloned());
        self.text = text;
        self.spans = spans_of(&next);
    }

    /// The spans brought into order, apart, inside the text and into range (a project file can hold anything).
    pub fn check_spans(&mut self) {
        let styles = self.char_styles();
        self.spans = spans_of(&styles);
    }
}

/// One style per character of a text of `count` characters, from `spans` (a later span wins where they overlap).
fn styles_of(spans: &[TextSpan], count: usize) -> Vec<SpanStyle> {
    let mut styles = vec![SpanStyle::default(); count];
    for span in spans.iter().take(MAX_TEXT_SPANS) {
        let end = (span.end as usize).min(count);
        let start = (span.start as usize).min(end);
        let style = span.style.clone().checked();
        for slot in styles.get_mut(start..end).into_iter().flatten() {
            *slot = style.clone();
        }
    }
    styles
}

/// The spans of per-character styles: each run of characters with the same style that sets something.
fn spans_of(styles: &[SpanStyle]) -> Vec<TextSpan> {
    let mut spans: Vec<TextSpan> = Vec::new();
    for (index, style) in styles.iter().enumerate() {
        if style.is_empty() {
            continue;
        }
        let Ok(at) = u32::try_from(index) else {
            break;
        };
        match spans.last_mut() {
            Some(last) if last.end == at && last.style == *style => last.end = at.saturating_add(1),
            _ => {
                if spans.len() >= MAX_TEXT_SPANS {
                    break;
                }
                spans.push(TextSpan {
                    start: at,
                    end: at.saturating_add(1),
                    style: style.clone(),
                });
            }
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> MenuText {
        serde_json::from_value(serde_json::json!({"text": value, "x": 0, "y": 0})).unwrap()
    }

    fn sized(size: u16) -> impl Fn(&mut SpanStyle) {
        move |style: &mut SpanStyle| style.size = Some(size)
    }

    #[test]
    fn characters_get_their_own_formatting() {
        let mut t = text("Hello world");
        t.style_chars(6..11, &sized(48));
        assert_eq!(
            t.spans,
            vec![TextSpan {
                start: 6,
                end: 11,
                style: SpanStyle {
                    size: Some(48),
                    ..SpanStyle::default()
                }
            }]
        );
        assert_eq!(t.span_style_at(5), SpanStyle::default());
        assert_eq!(t.span_style_at(6).size, Some(48));
        // Bold over part of it splits the span; the same again joins the runs.
        t.style_chars(8..9, &|style| style.bold = Some(true));
        assert_eq!(t.spans.len(), 3);
        t.style_chars(8..9, &|style| style.bold = None);
        assert_eq!(t.spans.len(), 1);
        // Set for the whole text, the field leaves the spans.
        t.style_all_chars(&|style| style.size = None);
        assert!(t.spans.is_empty());
    }

    #[test]
    fn spans_follow_the_text_as_it_is_edited() {
        let mut t = text("Hello world");
        t.style_chars(6..11, &sized(48));
        // Typed inside the span: the new characters take its formatting.
        t.retext("Hello wonderful world".to_string());
        assert_eq!((t.spans[0].start, t.spans[0].end), (6, 21));
        // Typed before it: it moves.
        t.retext("Oh, Hello wonderful world".to_string());
        assert_eq!((t.spans[0].start, t.spans[0].end), (10, 25));
        // Its characters taken out: it goes.
        t.retext("Oh, Hello ".to_string());
        assert!(t.spans.is_empty());
        // Japanese: counted in characters.
        let mut t = text("こんにちは世界");
        t.style_chars(5..7, &|style| style.color = Some([255, 0, 0]));
        t.retext("こんにちは、世界".to_string());
        assert_eq!((t.spans[0].start, t.spans[0].end), (6, 8));
    }

    #[test]
    fn spans_from_a_file_are_put_in_order() {
        let mut t = text("abcdef");
        t.spans = vec![
            TextSpan {
                start: 4,
                end: 99,
                style: SpanStyle {
                    size: Some(9999),
                    ..SpanStyle::default()
                },
            },
            TextSpan {
                start: 3,
                end: 1,
                style: SpanStyle {
                    bold: Some(true),
                    ..SpanStyle::default()
                },
            },
            TextSpan {
                start: 0,
                end: 2,
                style: SpanStyle::default(),
            },
        ];
        t.check_spans();
        assert_eq!(t.spans.len(), 1);
        assert_eq!((t.spans[0].start, t.spans[0].end), (4, 6));
        assert_eq!(t.spans[0].style.size, Some(MAX_TEXT_SIZE));
    }

    #[test]
    fn a_span_is_kept_in_a_project_file() {
        let mut t = text("Hello");
        t.style_chars(0..2, &|style| {
            style.color = Some([1, 2, 3]);
            style.font = Some("Arial".to_string());
        });
        let json = serde_json::to_value(&t).unwrap();
        assert_eq!(
            json["spans"],
            serde_json::json!([{"start": 0, "end": 2, "font": "Arial", "color": [1, 2, 3]}])
        );
        let back: MenuText = serde_json::from_value(json).unwrap();
        assert_eq!(back, t);
        // A text without spans keeps the file as it was.
        let plain = serde_json::to_value(text("Hi")).unwrap();
        assert!(plain.get("spans").is_none());
    }
}
