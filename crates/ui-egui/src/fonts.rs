use egui::{Context, FontData, FontDefinitions, FontFamily, FontId};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// 日本語の書体の候補（ファイル、TTC の中の番号）。先に見つかったものを使う。Windows では Encore と同じ
/// MS UI Gothic（[`ENCORE_JAPANESE`]）が先に使われ、これはそれに無い字を補う。
///
/// UI 向けの書体（Meiryo UI、Yu Gothic UI）を先に置く。
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

/// Encore の画面の英数字の書体の候補（M6.450 でフローチャートに、M6.461 で画面全体に）。Encore CS6 は名前も
/// ボタンもプロパティも Tahoma の 11 ピクセルで描く。OS の書体を読むだけで、同梱はしない。
const ENCORE_LATIN: &[(&str, u32)] = &[
    (r"C:\Windows\Fonts\tahoma.ttf", 0),
    ("/System/Library/Fonts/Supplemental/Tahoma.ttf", 0),
];

/// Encore の画面の日本語の書体の候補（M6.458、M6.461）。Encore の日本語は、Windows が Tahoma の代わりに
/// MS UI Gothic で描いたもので、英数字の 11 に対して 12 の大きさ（「グロリアス」の幅が 45、Meiryo UI の 11
/// では 41）。msgothic.ttc の 1 番目が MS UI Gothic。
const ENCORE_JAPANESE: &[(&str, u32)] = &[(r"C:\Windows\Fonts\msgothic.ttc", 1)];

/// 英数字の大きさに対する日本語の大きさ（Encore: 11 に 12）。
const JAPANESE_SCALE: f32 = 12.0 / 11.0;

/// Encore の英数字の書体（Tahoma）が見つかり、画面の文字をその大きさ（11）で描くか。
static ENCORE_FONTS: AtomicBool = AtomicBool::new(false);

/// 画面の文字の大きさの基準：Encore の書体があれば Encore と同じ 11、なければ今までの 12。
pub(crate) fn body_size() -> f32 {
    if ENCORE_FONTS.load(Ordering::Relaxed) {
        11.0
    } else {
        12.0
    }
}

/// フローチャートの文字の書体（画面の文字と同じ組）。
pub(crate) fn chart_font(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Encore の書体の描き方：横にも画素にそろえ、字を画素の境目に置く（M6.457）。Encore（Windows の GDI）の
/// Tahoma のように縦の線が 1 画素でくっきりする。egui の既定は横をそろえず、11 ピクセルの縦の線が 2 画素に
/// ぼける。
fn encore_tweak(scale: f32) -> egui::epaint::text::FontTweak {
    use egui::epaint::text::{FontTweak, HintingTarget, SmoothHinting};
    FontTweak {
        scale,
        hinting: Some(true),
        hinting_target: HintingTarget::Smooth(SmoothHinting {
            light: false,
            symmetric_rendering: false,
            preserve_linear_metrics: false,
        }),
        subpixel_binning: Some(false),
        ..FontTweak::default()
    }
}

/// 候補のうち最初に読めたフォント。
fn first_font(candidates: &[(&str, u32)]) -> Option<FontData> {
    candidates.iter().find_map(|(path, index)| {
        let mut bytes = std::fs::read(path).ok().filter(|bytes| !bytes.is_empty())?;
        let _ = hide_bitmaps(&mut bytes, *index);
        let mut data = FontData::from_owned(bytes);
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

/// 見つかった書体：日本語（Meiryo UI など）、Encore の英数字（Tahoma）と日本語（MS UI Gothic）。
#[derive(Default)]
struct Found {
    japanese: Option<FontData>,
    latin: Option<FontData>,
    encore_japanese: Option<FontData>,
}

/// egui の既定の書体に、Encore の書体と日本語の書体を足した定義。
///
/// 文章用（Proportional）は Encore と同じく英数字が Tahoma、日本語が MS UI Gothic（英数字の 12/11 倍）で、どちらにも
/// 無い字を日本語の書体（Meiryo UI など）と egui の内蔵フォントで補う。Tahoma が無ければ今までどおり日本語の書体を
/// いちばん先にして、英数字も同じ書体で描く（英数字だけ egui の内蔵フォントになると、「本編A」のように和文と欧文の
/// 基準線がずれて読みにくい）。等幅（タイムコードなど）は内蔵の等幅フォントを先に残し、日本語だけこの書体で補う。
fn font_definitions(found: Found) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut front: Vec<String> = Vec::new();
    let encore = found.latin.is_some();
    for (name, data, tweak) in [
        ("encore", found.latin, encore_tweak(1.0)),
        (
            "encore-ja",
            found.encore_japanese,
            encore_tweak(JAPANESE_SCALE),
        ),
    ] {
        // The Japanese one only after Tahoma: alone, it would draw the letters too, a size larger.
        if let Some(data) = data.filter(|_| encore) {
            let _ = fonts
                .font_data
                .insert(name.to_owned(), Arc::new(data.tweak(tweak)));
            front.push(name.to_owned());
        }
    }
    if let Some(data) = found.japanese {
        let _ = fonts.font_data.insert("jp".to_owned(), Arc::new(data));
        front.push("jp".to_owned());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push("jp".to_owned());
    }
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    for (at, name) in front.into_iter().enumerate() {
        proportional.insert(at, name);
    }
    fonts
}

/// 日本語の書体と Encore の書体を入れる。[`crate::apply_theme`] より先に呼ぶ（文字の大きさが書体で決まる）。
pub fn install_fonts(ctx: &Context) {
    let found = Found {
        japanese: first_font(FONT_CANDIDATES),
        latin: first_font(ENCORE_LATIN),
        encore_japanese: first_font(ENCORE_JAPANESE),
    };
    ENCORE_FONTS.store(found.latin.is_some(), Ordering::Relaxed);
    ctx.set_fonts(font_definitions(found));
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn family(fonts: &FontDefinitions, family: &FontFamily) -> Vec<String> {
        fonts.families.get(family).cloned().unwrap_or_default()
    }

    fn data() -> Option<FontData> {
        // Any bytes do for the order; the fonts are parsed only when text is laid out.
        Some(FontData::from_static(b"font"))
    }

    #[test]
    fn encore_fonts_come_first_then_the_japanese_one() {
        let fonts = font_definitions(Found {
            japanese: data(),
            latin: data(),
            encore_japanese: data(),
        });
        let proportional = family(&fonts, &FontFamily::Proportional);
        assert_eq!(
            proportional.get(..3),
            Some(&["encore", "encore-ja", "jp"].map(String::from)[..])
        );
        let monospace = family(&fonts, &FontFamily::Monospace);
        assert_eq!(monospace.last().map(String::as_str), Some("jp"));
        assert!(!monospace.contains(&"encore".to_owned()));
    }

    #[test]
    fn without_tahoma_the_japanese_font_draws_everything() {
        // MS UI Gothic alone would draw the letters a size larger: it is left out with Tahoma.
        let fonts = font_definitions(Found {
            japanese: data(),
            latin: None,
            encore_japanese: data(),
        });
        let proportional = family(&fonts, &FontFamily::Proportional);
        assert_eq!(proportional.first().map(String::as_str), Some("jp"));
        assert!(!proportional.contains(&"encore-ja".to_owned()));
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

    #[test]
    fn nothing_found_keeps_egui_fonts() {
        let before = family(&FontDefinitions::default(), &FontFamily::Proportional);
        let fonts = font_definitions(Found::default());
        assert_eq!(family(&fonts, &FontFamily::Proportional), before);
    }
}
