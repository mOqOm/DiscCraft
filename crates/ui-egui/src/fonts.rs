//! The interface's fonts: craft-ui's font stack, the one every Craft app installs (M9.66), with the suite's faces since
//! M9.68 (Encore's Tahoma and MS UI Gothic are no longer used).
//!
//! The stack's own order: the suite's interface faces (Inter, with its semibold and egui's monospace), the craft-fonts
//! faces a build embeds (`build.rs`; release builds embed them) for the UI language and every other script the
//! interface offers, then the installed system fonts (Japanese on a machine whose craft-fonts have no Japanese interface
//! face) and the emoji faces.

use craft_ui::fonts::{EmbeddedFace, I18nScripts, Stack, UiFonts};
use egui::{Context, FontData, FontId, Id};
use std::sync::OnceLock;

/// craft-fonts faces (OFL-1.1); empty unless built with a craft-fonts checkout (see `build.rs`).
static CRAFT_FONTS: &[EmbeddedFace] = include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));

/// The languages the interface offers, whose scripts the stack keeps faces for.
const LANGUAGES: [&str; 2] = ["en", "ja"];

/// The name of the OS's Japanese interface face in the stack.
const JP: &str = "jp";

/// 日本語の書体の候補（ファイル、TTC の中の番号）。先に見つかったものを、どの書体の組でも最後の頼りとして
/// 足す（FilmCraft と同じく、Craft の書体に無い日本語を描く）。UI 向けの書体（Meiryo UI、Yu Gothic UI）を先に置く。
const FONT_CANDIDATES: &[(&str, u32)] = &[
    (r"C:\Windows\Fonts\meiryo.ttc", 2),  // Meiryo UI
    (r"C:\Windows\Fonts\YuGothM.ttc", 1), // Yu Gothic UI
    (r"C:\Windows\Fonts\YuGothR.ttc", 0), // Yu Gothic
    (r"C:\Windows\Fonts\msgothic.ttc", 0),
    ("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc", 0),
    ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
    ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
    ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 0),
];

/// The interface's text size, as in the other Craft apps (12 px).
pub(crate) fn body_size() -> f32 {
    12.0
}

/// フローチャートの文字の書体（画面の文字と同じ組）。
pub(crate) fn chart_font(size: f32) -> FontId {
    FontId::proportional(size)
}

/// 候補のうち最初に読めたフォント。一度だけ読み、アプリが閉じるまで持つ（言語を替えて入れ直すときも同じ中身）。
fn first_font(candidates: &[(&str, u32)]) -> Option<FontData> {
    candidates.iter().find_map(|(path, index)| {
        let mut bytes = std::fs::read(path).ok().filter(|bytes| !bytes.is_empty())?;
        let _ = hide_bitmaps(&mut bytes, *index);
        let mut data = FontData::from_static(Box::leak(bytes.into_boxed_slice()));
        data.index = *index;
        Some(data)
    })
}

/// Where face `index` of a font file starts: a collection's (`ttcf`) face, or a single font's 0.
fn face_offset(bytes: &[u8], index: u32) -> Option<usize> {
    let u32_at = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(
            bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
        ))
    };
    if bytes.get(..4) == Some(b"ttcf") {
        let count = u32_at(8)?;
        if index >= count {
            return None;
        }
        let entry = 12_usize.checked_add(usize::try_from(index).ok()?.checked_mul(4)?)?;
        usize::try_from(u32_at(entry)?).ok()
    } else {
        (index == 0).then_some(0)
    }
}

/// Hide face `index`'s black-and-white bitmaps (the `EBDT` and `EBLC` tables, which MS UI Gothic has for its small
/// sizes) from egui by renaming them in the table directory, in memory only (M6.466). egui takes any bitmap glyph
/// for a colour one and draws it in its own white, whatever colour the text is: black in a light field, grey when
/// disabled, red in a warning. Without them egui draws the outlines, in the text's colour. The new names keep the
/// directory's order ("EBDu" after "EBDT", "EBLc" after "EBLC"). Whether a table was renamed.
fn hide_bitmaps(bytes: &mut [u8], index: u32) -> bool {
    let Some(start) = face_offset(bytes, index) else {
        return false;
    };
    let tables = start
        .checked_add(4)
        .and_then(|at| bytes.get(at..at.checked_add(2)?))
        .and_then(|count| <[u8; 2]>::try_from(count).ok())
        .map_or(0, |count| usize::from(u16::from_be_bytes(count)));
    let mut renamed = false;
    for table in 0..tables {
        let Some(record) = table
            .checked_mul(16)
            .and_then(|at| at.checked_add(start)?.checked_add(12))
        else {
            break;
        };
        let new_last = match bytes.get(record..record.saturating_add(4)) {
            Some(b"EBDT") => b'u',
            Some(b"EBLC") => b'c',
            Some(_) => continue,
            None => break,
        };
        if let Some(last) = bytes.get_mut(record.saturating_add(3)) {
            *last = new_last;
            renamed = true;
        }
    }
    renamed
}

/// The OS's Japanese interface face, read once.
fn japanese() -> Option<&'static FontData> {
    static FOUND: OnceLock<Option<FontData>> = OnceLock::new();
    FOUND.get_or_init(|| first_font(FONT_CANDIDATES)).as_ref()
}

/// The interface's font stack for the UI language `language` (`en`, `ja`): the suite's, as every Craft app has it, with
/// the OS's Japanese interface face `japanese` last in every family (before the system fonts), as FilmCraft adds its
/// Japanese face.
fn ui_fonts(language: &str, japanese: Option<&FontData>) -> Stack {
    let mut stack = UiFonts::new(&I18nScripts)
        .craft_faces(CRAFT_FONTS)
        .languages(&LANGUAGES)
        .ui_language(language)
        .stack();
    if let Some(japanese) = japanese {
        let families: Vec<_> = stack
            .weights()
            .iter()
            .map(|(family, _)| family.clone())
            .collect();
        for family in families {
            let end = stack.chain(&family).len();
            stack.insert(&family, end, JP, japanese.clone());
        }
    }
    stack
}

/// Where a context keeps the language its fonts were installed for.
fn installed_language_id() -> Id {
    Id::unique("disccraft-fonts-language")
}

/// Installs the stack for `language` through the UI runtime (M9.54), which notes what it holds for the framework's own
/// text (the dock's tabs, the menu bar).
fn install_for(ctx: &Context, language: &str) {
    ui_fonts(language, japanese()).install(ctx);
    ctx.data_mut(|data| data.insert_temp(installed_language_id(), language.to_owned()));
    ctx.request_repaint();
}

/// The interface's fonts. Call it before [`crate::apply_theme`] (the text sizes go with the fonts).
pub fn install_fonts(ctx: &Context) {
    install_for(ctx, crate::workspace::load_language().code());
}

/// Installs the fonts again when the interface language changed since they were installed in `ctx` (the stack orders
/// the faces for the UI language). A context the fonts were never installed in (a test's) is left alone.
pub(crate) fn follow_language(ctx: &Context, language: crate::i18n::Language) {
    let installed = ctx.data(|data| data.get_temp::<String>(installed_language_id()));
    if installed.is_some_and(|code| code != language.code()) {
        install_for(ctx, language.code());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_ui::fonts::FontRef;

    fn data() -> FontData {
        // Any bytes do for the order; the fonts are parsed only when text is laid out.
        FontData::from_static(b"font")
    }

    #[test]
    fn the_suite_faces_lead_in_every_language() {
        for language in ["en", "ja", "xx"] {
            let stack = ui_fonts(language, Some(&data()));
            let proportional = stack.chain(&FontRef::proportional());
            assert_eq!(
                proportional.first().map(String::as_str),
                Some("Inter-Regular"),
                "{language}: {proportional:?}"
            );
            for gone in ["encore", "encore-ja"] {
                assert!(
                    !proportional.iter().any(|name| name == gone),
                    "{language}: {proportional:?}"
                );
            }
            // The Japanese face is the last of every family's own faces.
            for family in [FontRef::proportional(), FontRef::monospace()] {
                assert_eq!(
                    stack.chain(&family).last().map(String::as_str),
                    Some(JP),
                    "{language}: {:?}",
                    stack.chain(&family)
                );
            }
        }
        // Without a Japanese face the suite's stack is as it is.
        let stack = ui_fonts("ja", None);
        assert!(
            !stack
                .chain(&FontRef::proportional())
                .contains(&JP.to_owned())
        );
    }

    /// A context with the interface's fonts installed for `language`, after the passes that apply them.
    fn context(language: &str) -> Context {
        let ctx = Context::default();
        ui_fonts(language, japanese()).install(&ctx);
        for _ in 0..2 {
            let mut output = ctx.run_ui(Default::default(), |_| {});
            output.textures_delta.clear();
        }
        ctx
    }

    /// The menus' Japanese, the arrows and marks draw where the OS has a Japanese face (every Windows and macOS does).
    #[test]
    fn the_interface_draws_every_character_of_its_japanese_menus() {
        if !cfg!(any(windows, target_os = "macos")) {
            return;
        }
        let ctx = context("ja");
        let id = FontId::proportional(body_size());
        for character in "ファイル(F) 編集(E) タイムライン(T) ▸ ▼ ✓ × … 「」".chars()
        {
            assert!(
                ctx.fonts_mut(|fonts| fonts.has_glyph(&id, character)),
                "missing U+{:04X}",
                u32::from(character)
            );
        }
    }

    /// A font face's start: version, table count, the search fields, then a 16-byte record per tag.
    fn face(tags: &[&[u8; 4]]) -> Vec<u8> {
        let mut out = vec![0, 1, 0, 0];
        out.extend_from_slice(&u16::try_from(tags.len()).unwrap_or(0).to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        for tag in tags {
            out.extend_from_slice(*tag);
            out.extend_from_slice(&[0; 12]);
        }
        out
    }

    fn tags(bytes: &[u8], start: usize, count: usize) -> Vec<String> {
        (0..count)
            .filter_map(|table| bytes.get(start + 12 + 16 * table..start + 16 + 16 * table))
            .map(|tag| String::from_utf8_lossy(tag).into_owned())
            .collect()
    }

    #[test]
    fn bitmaps_are_hidden_in_the_chosen_face_only() {
        let first = face(&[b"EBDT", b"cmap"]);
        let second = face(&[b"DSIG", b"EBDT", b"EBLC", b"EBSC", b"GDEF"]);
        let mut file = b"ttcf".to_vec();
        file.extend_from_slice(&[0, 1, 0, 0]);
        file.extend_from_slice(&2_u32.to_be_bytes());
        let first_at = 20_u32;
        let second_at = first_at + u32::try_from(first.len()).unwrap_or(0);
        file.extend_from_slice(&first_at.to_be_bytes());
        file.extend_from_slice(&second_at.to_be_bytes());
        file.extend_from_slice(&first);
        file.extend_from_slice(&second);
        assert!(hide_bitmaps(&mut file, 1));
        let second_start = usize::try_from(second_at).unwrap_or(0);
        // Renamed in place, still in the directory's order.
        assert_eq!(
            tags(&file, second_start, 5),
            ["DSIG", "EBDu", "EBLc", "EBSC", "GDEF"]
        );
        assert_eq!(tags(&file, 20, 2), ["EBDT", "cmap"]);
        // A face the file does not have, a single font, a cut file: nothing breaks.
        assert!(!hide_bitmaps(&mut file, 2));
        let mut single = face(&[b"EBLC"]);
        assert!(hide_bitmaps(&mut single, 0));
        assert_eq!(tags(&single, 0, 1), ["EBLc"]);
        let mut cut = face(&[b"EBDT", b"EBLC"]);
        // Cut in the first record's name.
        cut.truncate(14);
        assert!(!hide_bitmaps(&mut cut, 0));
        assert!(!hide_bitmaps(&mut [], 0));
    }
}
