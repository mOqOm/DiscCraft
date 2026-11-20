//! Paragraph text: breaking the lines of a text at a width (Encore's area text, drawn by dragging with the text
//! tool), with Encore's kinsoku sets (禁則処理: なし / 弱い禁則 / 強い禁則).
//!
//! A line may break after a space, and next to a Japanese (full-width) character. Kinsoku then forbids some of
//! those places: no line starts with a closing bracket, a punctuation mark or an iteration mark, none ends with
//! an opening bracket (弱い禁則); 強い禁則 also keeps small kana and the long vowel mark off the head of a line.
//! A forbidden place is passed over by Encore's 禁則方法: taking the characters that may not start a line into the
//! line above when their blank room makes way for them (追い込み), or taking the character before them to the
//! next line as well (追い出し). With ぶら下がり a full-width comma or full stop that does not fit hangs past the
//! end of the line. The sets are the ones JIS X 4051 names as line-start and line-end prohibited characters.

use disccraft_project::{TextFormat, TextKinsoku, TextKinsokuMethod};

/// How a paragraph text's lines are broken: the kinsoku set, how a forbidden place is passed over, and whether
/// commas and full stops hang.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rules {
    /// The characters a line does not start or end with.
    pub(crate) kinsoku: TextKinsoku,
    /// Taken into the line above or the character before them taken down.
    pub(crate) method: TextKinsokuMethod,
    /// A full-width comma or full stop hangs past the end of a line (ぶら下がり).
    pub(crate) hanging: bool,
}

impl Rules {
    /// The rules of a text's format.
    pub(crate) fn of(format: &TextFormat) -> Rules {
        Rules {
            kinsoku: format.kinsoku,
            method: format.kinsoku_method,
            hanging: format.hanging_punctuation,
        }
    }
}

impl From<TextKinsoku> for Rules {
    /// A kinsoku set alone: the character before always goes down (追い出しのみ), nothing hangs.
    fn from(kinsoku: TextKinsoku) -> Rules {
        Rules {
            kinsoku,
            method: TextKinsokuMethod::PushOutOnly,
            hanging: false,
        }
    }
}

/// How far past the end of a line characters taken into it (追い込み) may reach, as a share of the line's
/// average character: the blank half of a full-width comma, full stop or closing bracket, and a little.
const PUSH_IN_ROOM: f32 = 0.6;

/// A full-width comma or full stop: it may hang past the end of a line (ぶら下がり).
fn hangs(c: char) -> bool {
    matches!(c, '、' | '。' | '，' | '．')
}

/// The most lines a wrapped text is broken into (a text is at most a few thousand characters).
const MAX_LINES: usize = 500;

/// A character that never starts a line under 弱い禁則: closing brackets and quotes, commas and full stops,
/// the middle dot, colons, question and exclamation marks, iteration marks.
fn not_at_start(c: char) -> bool {
    matches!(
        c,
        '、' | '。'
            | '，'
            | '．'
            | '・'
            | '：'
            | '；'
            | '？'
            | '！'
            | '」'
            | '』'
            | '）'
            | '］'
            | '｝'
            | '〕'
            | '〉'
            | '》'
            | '】'
            | '’'
            | '”'
            | 'ゝ'
            | 'ゞ'
            | 'ヽ'
            | 'ヾ'
            | '々'
            | '‥'
            | '…'
            | ','
            | '.'
            | ')'
            | ']'
            | '}'
            | '!'
            | '?'
            | ':'
            | ';'
    )
}

/// What 強い禁則 adds to [`not_at_start`]: small kana and the long vowel mark.
fn not_at_start_strong(c: char) -> bool {
    matches!(
        c,
        'ぁ' | 'ぃ'
            | 'ぅ'
            | 'ぇ'
            | 'ぉ'
            | 'っ'
            | 'ゃ'
            | 'ゅ'
            | 'ょ'
            | 'ゎ'
            | 'ァ'
            | 'ィ'
            | 'ゥ'
            | 'ェ'
            | 'ォ'
            | 'ッ'
            | 'ャ'
            | 'ュ'
            | 'ョ'
            | 'ヮ'
            | 'ヵ'
            | 'ヶ'
            | 'ー'
    )
}

/// A character that never ends a line: opening brackets and quotes.
fn not_at_end(c: char) -> bool {
    matches!(
        c,
        '「' | '『'
            | '（'
            | '［'
            | '｛'
            | '〔'
            | '〈'
            | '《'
            | '【'
            | '‘'
            | '“'
            | '('
            | '['
            | '{'
    )
}

/// A character set in a full square (kana, kanji, full-width forms and CJK punctuation): a line may break at
/// either side of it.
pub(crate) fn wide(c: char) -> bool {
    matches!(
        u32::from(c),
        0x2E80..=0x9FFF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF01..=0xFF60 | 0xFFE0..=0xFFE6
            | 0x2_0000..=0x3_FFFF
    )
}

/// A mark that belongs to the character before it (combining marks, voicing marks, variation selectors, the
/// joiner): never parted from it.
fn joins(c: char) -> bool {
    matches!(
        u32::from(c),
        0x0300..=0x036F | 0x3099 | 0x309A | 0xFE00..=0xFE0F | 0x200D | 0xE_0100..=0xE_01EF
    )
}

/// Whether a line may break between `before` and `after`.
pub(crate) fn may_break(before: char, after: char, kinsoku: TextKinsoku) -> bool {
    if joins(after) || before == '\u{200D}' {
        return false;
    }
    // At either end of a run of spaces (the spaces themselves are left out of both lines).
    let place = ((before == ' ') != (after == ' ')) || wide(before) || wide(after);
    if !place {
        return false;
    }
    match kinsoku {
        TextKinsoku::None => true,
        TextKinsoku::Weak => !not_at_start(after) && !not_at_end(before),
        TextKinsoku::Strong => {
            !not_at_start(after) && !not_at_start_strong(after) && !not_at_end(before)
        }
    }
}

/// `text` with line breaks added so that no line is wider than `limit`, as far as the breaking rules allow
/// (a word or a run of forbidden places longer than the limit is cut where the limit is). `measure` gives the
/// width of one line. The spaces a line breaks at are left out. A `limit` that is not a positive number leaves
/// the text as it is.
pub(crate) fn wrap(
    text: &str,
    limit: f32,
    rules: impl Into<Rules>,
    measure: &dyn Fn(&str) -> f32,
) -> String {
    let lines = wrap_lines(text, limit, rules, &|range| {
        text.get(range).map_or(f32::INFINITY, measure)
    });
    joined(text, &lines)
}

/// [`wrap`] as the lines it makes: the byte range of `text` each one is (without the spaces it breaks at), `measure`
/// giving the width of a range of `text` (M6.389: so that characters with formatting of their own are measured as
/// they are set).
pub(crate) fn wrap_lines(
    text: &str,
    limit: f32,
    rules: impl Into<Rules>,
    measure: &dyn Fn(std::ops::Range<usize>) -> f32,
) -> Vec<std::ops::Range<usize>> {
    let rules = rules.into();
    let mut lines = Vec::new();
    let mut breaks = 0_usize;
    let mut base = 0_usize;
    for source in text.split('\n') {
        let end = base.saturating_add(source.len());
        wrap_paragraph(
            text,
            base..end,
            (limit, rules),
            measure,
            &mut breaks,
            &mut lines,
        );
        base = end.saturating_add(1);
    }
    lines
}

/// The lines of the paragraph `paragraph` (a byte range of `text`) appended to `lines`; `breaks` counts the lines
/// broken so far (at most [`MAX_LINES`]). A `limit` that is not a positive number leaves it one line.
fn wrap_paragraph(
    text: &str,
    paragraph: std::ops::Range<usize>,
    (limit, rules): (f32, Rules),
    measure: &dyn Fn(std::ops::Range<usize>) -> f32,
    breaks: &mut usize,
    lines: &mut Vec<std::ops::Range<usize>>,
) {
    let end = paragraph.end;
    if !(limit.is_finite() && limit > 0.0) {
        lines.push(paragraph);
        return;
    }
    let mut start = paragraph.start;
    loop {
        let rest = text.get(start..end).unwrap_or("");
        if *breaks >= MAX_LINES || rest.is_empty() || measure(start..end) <= limit {
            lines.push(start..end);
            return;
        }
        let cut = break_at(rest, start, limit, rules, measure);
        let (head, tail) = rest.split_at_checked(cut).unwrap_or((rest, ""));
        lines.push(start..start.saturating_add(head.trim_end_matches(' ').len()));
        let tail = tail.trim_start_matches(' ');
        if tail.is_empty() {
            return;
        }
        start = end.saturating_sub(tail.len());
        *breaks += 1;
    }
}

/// The lines of `text` (byte ranges) one after another, each after a line break.
pub(crate) fn joined(text: &str, lines: &[std::ops::Range<usize>]) -> String {
    let mut out = String::with_capacity(text.len().saturating_add(16));
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(text.get(line.clone()).unwrap_or(""));
    }
    out
}

/// A paragraph text set justified (Encore's 均等配置（最終行左揃え）): `text` wrapped at `limit` as [`wrap`] does, and
/// the room put after characters of every line but each paragraph's last so that it reaches `limit`, in 1/1000
/// of `em` (as tracking is). A line of words without Japanese gets it at its spaces; any other line between every
/// two characters (never between a character and a mark joined to it).
pub(crate) fn justify(
    text: &str,
    limit: f32,
    rules: Rules,
    measure: &dyn Fn(&str) -> f32,
    em: f32,
) -> (String, Vec<(std::ops::Range<usize>, f32)>) {
    let (out, room, _) = justify_lines(
        text,
        limit,
        rules,
        &|range| text.get(range).map_or(f32::INFINITY, measure),
        em,
    );
    (out, room)
}

/// [`justify`] with `measure` giving the width of a byte range of `text` (as [`wrap_lines`]), and the range of
/// `text` each line is.
#[allow(clippy::type_complexity)]
pub(crate) fn justify_lines(
    text: &str,
    limit: f32,
    rules: Rules,
    measure: &dyn Fn(std::ops::Range<usize>) -> f32,
    em: f32,
) -> (
    String,
    Vec<(std::ops::Range<usize>, f32)>,
    Vec<std::ops::Range<usize>>,
) {
    let mut out = String::with_capacity(text.len().saturating_add(16));
    let mut room = Vec::new();
    let mut ranges = Vec::new();
    let mut base = 0_usize;
    for (index, paragraph) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let end = base.saturating_add(paragraph.len());
        let mut lines = Vec::new();
        wrap_paragraph(text, base..end, (limit, rules), measure, &mut 0, &mut lines);
        base = end.saturating_add(1);
        let last = lines.len().saturating_sub(1);
        for (at, range) in lines.iter().enumerate() {
            if at > 0 {
                out.push('\n');
            }
            ranges.push(range.clone());
            let line = text.get(range.clone()).unwrap_or("");
            let start = out.len();
            out.push_str(line);
            let spare = limit - measure(range.clone());
            if at == last || !(spare.is_finite() && spare > 0.0 && em > 0.0) {
                continue;
            }
            let chars: Vec<(usize, char)> = line.char_indices().collect();
            let words = line.contains(' ') && !line.chars().any(wide);
            let gaps: Vec<(usize, char)> = chars
                .iter()
                .enumerate()
                .filter(|(index, (_, c))| {
                    let next = chars.get(index + 1).map(|(_, next)| *next);
                    if words {
                        *c == ' '
                    } else {
                        next.is_some_and(|next| !joins(next)) && *c != '\u{200D}'
                    }
                })
                .map(|(_, at)| *at)
                .collect();
            if gaps.is_empty() {
                continue;
            }
            // `as` is exact for the gaps a line holds.
            let each = spare / gaps.len() as f32 / em * 1000.0;
            room.extend(gaps.into_iter().map(|(offset, c)| {
                let from = start.saturating_add(offset);
                (from..from.saturating_add(c.len_utf8()), each)
            }));
        }
    }
    (out, room, ranges)
}

/// Where the first line of `line` (which is wider than `limit`; at byte `offset` of the text `measure` measures
/// ranges of) ends: a byte offset into `line` behind at least one character.
fn break_at(
    line: &str,
    offset: usize,
    limit: f32,
    rules: Rules,
    measure: &dyn Fn(std::ops::Range<usize>) -> f32,
) -> usize {
    // The width of the first `end` bytes of the line.
    let head_width = |end: usize| measure(offset..offset.saturating_add(end));
    let kinsoku = rules.kinsoku;
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    // The byte offset behind the first `count` characters.
    let end_of = |count: usize| chars.get(count).map_or(line.len(), |(offset, _)| *offset);
    // The most characters that fit (at least one): the widths grow with the count, so a binary search.
    let (mut fits, mut over) = (1_usize, chars.len());
    while fits + 1 < over {
        let middle = fits + (over - fits) / 2;
        let width = head_width(end_of(middle));
        if width <= limit {
            fits = middle;
        } else {
            over = middle;
        }
    }
    let at = |index: usize| chars.get(index).map(|(_, c)| *c);
    // A line may end behind `count` characters: at the end, or where the rules let it break.
    let ends = |count: usize| match (count.checked_sub(1).and_then(at), at(count)) {
        (Some(_), None) => true,
        (Some(before), Some(after)) => may_break(before, after, kinsoku),
        _ => false,
    };
    // ぶら下がり: a comma or full stop that does not fit hangs past the end.
    if rules.hanging && at(fits).is_some_and(hangs) && ends(fits + 1) {
        return end_of(fits + 1);
    }
    // 追い込み: the characters that may not start a line taken into it (at most a few, up to the next place it
    // may end), `tight`: only as far as their blank room makes way for them.
    let push_in = |tight: bool| -> Option<usize> {
        let count = (fits + 1..=fits.saturating_add(3).min(chars.len()))
            .take_while(|count| {
                at(count - 1).is_some_and(|c| {
                    not_at_start(c) || (kinsoku == TextKinsoku::Strong && not_at_start_strong(c))
                })
            })
            .find(|count| ends(*count))?;
        if !tight {
            return Some(count);
        }
        let head = head_width(end_of(fits));
        let longer = head_width(end_of(count));
        let room = PUSH_IN_ROOM * head / fits.max(1) as f32;
        (longer - limit <= room).then_some(count)
    };
    // 追い出し: the last place at or before that where a line may break.
    let push_out = || {
        (1..=fits)
            .rev()
            .find(|count| *count < chars.len() && ends(*count))
    };
    if ends(fits) {
        return end_of(fits);
    }
    let count = match rules.method {
        TextKinsokuMethod::PushInFirst => push_in(true).or_else(push_out),
        TextKinsokuMethod::PushOutFirst => push_out().or_else(|| push_in(false)),
        TextKinsokuMethod::PushOutOnly => push_out(),
    };
    end_of(count.unwrap_or(fits))
}

#[cfg(test)]
mod tests {
    use super::{Rules, justify, may_break, wrap};
    use disccraft_project::{TextKinsoku, TextKinsokuMethod};

    /// The strong set with a method, and hanging commas and full stops or not.
    fn rules(method: TextKinsokuMethod, hanging: bool) -> Rules {
        Rules {
            kinsoku: TextKinsoku::Strong,
            method,
            hanging,
        }
    }

    /// As [`measure`], but a full-width comma, full stop or closing bracket as wide as its ink: 4.
    fn ink(line: &str) -> f32 {
        line.chars()
            .map(|c| match c {
                '、' | '。' | '」' => 4.0,
                c if c.is_ascii() => 5.0,
                _ => 10.0,
            })
            .sum()
    }

    /// M6.297: justified, every line but a paragraph's last gets the room it lacks: between every two
    /// characters of Japanese, at the spaces of words; nothing for a line that is full or the last.
    #[test]
    fn a_justified_line_gets_the_room_it_lacks() {
        let kinsoku = Rules::from(TextKinsoku::Strong);
        // Lines of four squares in a box of 45 (each 40 wide): 5 more each, over three gaps; em 10.
        let (text, room) = justify("あいうえおかきくけ\nこ", 45.0, kinsoku, &measure, 10.0);
        assert_eq!(text, "あいうえ\nおかきく\nけ\nこ");
        let gaps: Vec<(usize, f32)> = room
            .iter()
            .map(|(range, each)| (range.start, *each))
            .collect();
        let third = 5.0 / 3.0 / 10.0 * 1000.0;
        assert_eq!(
            gaps,
            vec![
                (0, third),
                (3, third),
                (6, third),
                (13, third),
                (16, third),
                (19, third)
            ]
        );
        // Words: the room at the spaces (two of them, 2.5 short: 125 / 1000 em each).
        let (text, room) = justify("ab cd ef ghij", 42.5, kinsoku, &measure, 10.0);
        assert_eq!(text, "ab cd ef\nghij");
        assert_eq!(room, vec![(2..3, 125.0), (5..6, 125.0)]);
        // A voicing mark is never parted from its kana: the room goes after the mark.
        let (_, room) = justify("か\u{3099}きく\u{3099}け", 35.0, kinsoku, &measure, 10.0);
        assert_eq!(room, vec![(3..6, 500.0)]);
    }

    /// M6.296: Encore's 禁則方法 and ぶら下がり. A full stop that would start a line is taken into the line above
    /// when its blank room makes way for it (追い込み優先), else the character before it goes down with it
    /// (追い出し); 追い出し優先 takes it in only when the line has no place to break; with ぶら下がり it hangs.
    #[test]
    fn a_full_stop_is_pushed_in_pushed_out_or_hung() {
        use TextKinsokuMethod::{PushInFirst, PushOutFirst, PushOutOnly};
        // Five squares to a line; the full stop's ink reaches 4 past the end, within its blank room.
        let text = "あいうえお。かき";
        assert_eq!(
            wrap(text, 50.0, rules(PushInFirst, false), &ink),
            "あいうえお。
かき"
        );
        assert_eq!(
            wrap(text, 50.0, rules(PushOutFirst, false), &ink),
            "あいうえ
お。かき"
        );
        assert_eq!(
            wrap(text, 50.0, rules(PushOutOnly, false), &ink),
            "あいうえ
お。かき"
        );
        // A small kana is a whole square: no room for it, it goes down with the character before.
        assert_eq!(
            wrap("あいうえおっか", 50.0, rules(PushInFirst, false), &ink),
            "あいうえ
おっか"
        );
        // A full stop and a closing bracket: both taken in only if both fit in the room; here they do not.
        assert_eq!(
            wrap("あいうえお。」か", 50.0, rules(PushInFirst, false), &ink),
            "あいうえ
お。」か"
        );
        // Square-wide punctuation (no blank room): pushed out first, as before.
        assert_eq!(
            wrap(text, 50.0, rules(PushInFirst, false), &measure),
            "あいうえ
お。かき"
        );
        // ぶら下がり: the full stop hangs whatever its width.
        assert_eq!(
            wrap(text, 50.0, rules(PushOutOnly, true), &measure),
            "あいうえお。
かき"
        );
        // A line with no place to break: 追い出しのみ breaks where the limit is, 追い出し優先 takes them in.
        assert_eq!(
            wrap("ー。ーかき", 10.0, rules(PushOutOnly, false), &measure),
            "ー
。
ー
か
き"
        );
        assert_eq!(
            wrap("ー。ーかき", 10.0, rules(PushOutFirst, false), &measure),
            "ー。ー
か
き"
        );
    }

    /// Every character as wide as a square of 10, a space and an ASCII letter 5.
    fn measure(line: &str) -> f32 {
        line.chars()
            .map(|c| if c.is_ascii() { 5.0 } else { 10.0 })
            .sum()
    }

    #[test]
    fn japanese_breaks_anywhere_but_where_kinsoku_forbids() {
        // Five squares to a line.
        assert_eq!(
            wrap("あいうえおかきくけこ", 50.0, TextKinsoku::Strong, &measure),
            "あいうえお\nかきくけこ"
        );
        // A full stop does not start a line: the character before it goes down with it.
        assert_eq!(
            wrap("あいうえお。かき", 50.0, TextKinsoku::Strong, &measure),
            "あいうえ\nお。かき"
        );
        // Without kinsoku it does.
        assert_eq!(
            wrap("あいうえお。かき", 50.0, TextKinsoku::None, &measure),
            "あいうえお\n。かき"
        );
        // An opening bracket does not end a line.
        assert_eq!(
            wrap("あいうえ「おか」き", 50.0, TextKinsoku::Weak, &measure),
            "あいうえ\n「おか」き"
        );
        // A small kana starts a line under the weak set, not under the strong one.
        assert_eq!(
            wrap("あいうえきゃく", 50.0, TextKinsoku::Weak, &measure),
            "あいうえき\nゃく"
        );
        assert_eq!(
            wrap("あいうえきゃく", 50.0, TextKinsoku::Strong, &measure),
            "あいうえ\nきゃく"
        );
        assert!(!may_break('カ', 'ー', TextKinsoku::Strong));
        assert!(may_break('カ', 'ー', TextKinsoku::Weak));
    }

    #[test]
    fn words_break_at_spaces_and_long_ones_where_the_limit_is() {
        assert_eq!(
            wrap("play all the titles", 40.0, TextKinsoku::Strong, &measure),
            "play all\nthe\ntitles"
        );
        // A word longer than the line is cut.
        assert_eq!(
            wrap("abcdefghijkl", 20.0, TextKinsoku::Strong, &measure),
            "abcd\nefgh\nijkl"
        );
        // Lines the author broke stay; a text that fits, and a limit that is no width, are left alone.
        assert_eq!(
            wrap("あい\nうえおかきく", 50.0, TextKinsoku::Strong, &measure),
            "あい\nうえおかき\nく"
        );
        assert_eq!(wrap("あい", 50.0, TextKinsoku::Strong, &measure), "あい");
        assert_eq!(
            wrap("あいうえおか", 0.0, TextKinsoku::Strong, &measure),
            "あいうえおか"
        );
        assert_eq!(
            wrap("あいうえおか", f32::NAN, TextKinsoku::Strong, &measure),
            "あいうえおか"
        );
        // Narrower than one character: one character a line, and it ends.
        assert_eq!(
            wrap("あいう", 1.0, TextKinsoku::None, &measure),
            "あ\nい\nう"
        );
        // A voicing mark stays with its kana.
        assert!(!may_break('か', '\u{3099}', TextKinsoku::None));
    }
}
