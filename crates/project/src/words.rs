//! The language of the words DiscCraft makes up for a project and for the files it writes for it: the default
//! names of buttons, chapters, menus, slideshows, playlists and shapes, a chapter index's Previous / Next, and the
//! layer names of a menu's PSD or PhotoCraft document (its background, highlights, video thumbnails, texts and
//! pages). M6.433: they were partly English (the project's) and partly Japanese (the PSD's), so a menu or a
//! PhotoCraft file mixed the two; now they all follow the display language, which the session sets.
//!
//! A chapter added without a name keeps the name "Chapter N" in the project (as before, in every language); it is
//! a default name, written out in the words' language wherever it is shown ([`Words::chapter_name`]).

/// The language of the words DiscCraft makes up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Words {
    /// English (DiscCraft's own words, and the language of a project made without a display language).
    #[default]
    English,
    /// Japanese.
    Japanese,
}

impl Words {
    /// The words of the display language `code` ("ja", "en", …); English for a language DiscCraft has no words for.
    pub fn from_code(code: &str) -> Self {
        if code.trim().to_ascii_lowercase().starts_with("ja") {
            Self::Japanese
        } else {
            Self::English
        }
    }

    /// `english` or `japanese`.
    fn pick(self, english: &'static str, japanese: &'static str) -> &'static str {
        match self {
            Self::English => english,
            Self::Japanese => japanese,
        }
    }

    /// "Chapter 3" / "チャプター 3".
    pub fn chapter(self, number: impl std::fmt::Display) -> String {
        format!("{} {number}", self.pick("Chapter", "チャプター"))
    }

    /// The name of chapter `index` (0 for the first) named `name`: the name, or for a chapter without one of its own
    /// (empty, or the default "Chapter N"), "Chapter N" in these words.
    pub fn chapter_name(self, name: &str, index: usize) -> String {
        if name.trim().is_empty() {
            return self.chapter(index.saturating_add(1));
        }
        match default_chapter_number(name) {
            Some(number) => self.chapter(number),
            None => name.to_owned(),
        }
    }

    /// "Button 3" / "ボタン 3".
    pub fn button(self, number: impl std::fmt::Display) -> String {
        format!("{} {number}", self.pick("Button", "ボタン"))
    }

    /// "Menu 3" / "メニュー 3".
    pub fn menu(self, number: impl std::fmt::Display) -> String {
        format!("{} {number}", self.pick("Menu", "メニュー"))
    }

    /// A menu named without a number: "Menu" / "メニュー".
    pub fn menu_name(self) -> &'static str {
        self.pick("Menu", "メニュー")
    }

    /// "Folder 3" / "フォルダー 3".
    pub fn folder(self, number: impl std::fmt::Display) -> String {
        format!("{} {number}", self.pick("Folder", "フォルダー"))
    }

    /// A chapter playlist of the timeline `timeline`: "Main Chapter Playlist" / "Main チャプター再生リスト".
    pub fn chapter_playlist(self, timeline: &str) -> String {
        format!(
            "{timeline} {}",
            self.pick("Chapter Playlist", "チャプター再生リスト")
        )
    }

    /// "Slideshow 3" / "スライドショー 3".
    pub fn slideshow(self, number: impl std::fmt::Display) -> String {
        format!("{} {number}", self.pick("Slideshow", "スライドショー"))
    }

    /// "Playlist 3" / "再生リスト 3".
    pub fn playlist(self, number: impl std::fmt::Display) -> String {
        format!("{} {number}", self.pick("Playlist", "再生リスト"))
    }

    /// "Page 3" / "ページ 3" (a later page of a menu; Encore counts them from 0 for the first).
    pub fn page(self, number: impl std::fmt::Display) -> String {
        format!("{} {number}", self.pick("Page", "ページ"))
    }

    /// A chapter index's button to the page before: "Previous" / "前へ".
    pub fn previous(self) -> &'static str {
        self.pick("Previous", "前へ")
    }

    /// A chapter index's button to the page after: "Next" / "次へ".
    pub fn next(self) -> &'static str {
        self.pick("Next", "次へ")
    }

    /// The background layer of a menu's PSD: "Background" / "背景".
    pub fn background(self) -> &'static str {
        self.pick("Background", "背景")
    }

    /// A button's highlight layer (after its `(=1)` mark): "Highlight" / "ハイライト".
    pub fn highlight(self) -> &'static str {
        self.pick("Highlight", "ハイライト")
    }

    /// A button's video thumbnail layer (after its `(%)` mark): "Video" / "ビデオ".
    pub fn video(self) -> &'static str {
        self.pick("Video", "ビデオ")
    }

    /// A text layer ("Text: …"): "Text" / "テキスト".
    pub fn text(self) -> &'static str {
        self.pick("Text", "テキスト")
    }

    /// A picture whose file is gone: "Image" / "画像".
    pub fn image(self) -> &'static str {
        self.pick("Image", "画像")
    }

    /// "Rectangle" / "長方形".
    pub fn rectangle(self) -> &'static str {
        self.pick("Rectangle", "長方形")
    }

    /// "Rounded Rectangle" / "角丸長方形".
    pub fn rounded_rectangle(self) -> &'static str {
        self.pick("Rounded Rectangle", "角丸長方形")
    }

    /// "Ellipse" / "楕円".
    pub fn ellipse(self) -> &'static str {
        self.pick("Ellipse", "楕円")
    }
}

/// The number of a chapter's default name: "3" for "Chapter 3" (the name a chapter added without one gets), `None`
/// for a name of its own.
pub fn default_chapter_number(name: &str) -> Option<&str> {
    name.trim()
        .strip_prefix("Chapter ")
        .filter(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::{Words, default_chapter_number};

    /// The words follow the display language; a default chapter name is written in them, a chapter's own name is
    /// kept.
    #[test]
    fn words_follow_the_language() {
        let ja = Words::from_code("ja");
        let en = Words::from_code("en");
        assert_eq!(
            (ja, en, Words::from_code("de")),
            (Words::Japanese, Words::English, Words::English)
        );
        assert_eq!(ja.chapter_name("Chapter 7", 2), "チャプター 7");
        assert_eq!(ja.chapter_name("", 2), "チャプター 3");
        assert_eq!(ja.chapter_name("オープニング", 0), "オープニング");
        assert_eq!(en.chapter_name("Chapter 7", 2), "Chapter 7");
        assert_eq!(en.chapter_name(" ", 0), "Chapter 1");
        assert_eq!(ja.button(2), "ボタン 2");
        assert_eq!((ja.previous(), ja.next()), ("前へ", "次へ"));
        assert_eq!((en.previous(), en.next()), ("Previous", "Next"));
        assert_eq!(ja.page(1), "ページ 1");
        assert_eq!(default_chapter_number("Chapter 12"), Some("12"));
        assert_eq!(default_chapter_number("Chapter Twelve"), None);
        assert_eq!(default_chapter_number("Chapter "), None);
    }
}
