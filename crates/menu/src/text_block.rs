//! A text object as pieces to draw (each a layout with its own transform), so a vertical text (Encore's Vertical
//! Text tool) can mix upright and sideways runs in one block. A horizontal text is one piece.
//!
//! Vertical text, as Encore sets it: each line is a column, the columns run right to left; in a column, Japanese and
//! other full-width characters stand upright one under the other (laid out by FilmCraft's vertical layout, which
//! also takes the fonts' vertical forms of punctuation), and runs of other characters (Latin letters, digits,
//! half-width punctuation) are laid out as a line and turned a quarter clockwise, reading downwards.

use crate::draw::formatted_layout_in;
use disccraft_project::{TextAlign, TextFormat};
use filmcraft_text::{Layout, Mask, Xform, render};
use std::sync::Arc;

/// A layout and the transform that puts it in place.
pub(crate) type Piece = (Arc<Layout>, Xform);

/// The transform that leaves a layout where it is.
pub(crate) const IDENTITY: Xform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// The largest mask the pieces of one text are put together in (pixels), so a huge text cannot ask for any amount
/// of memory.
const MAX_MASK_PIXELS: usize = 8192 * 8192;

/// `outer` after `inner`: a point goes through `inner` first.
pub(crate) fn compose(outer: &Xform, inner: &Xform) -> Xform {
    let [a, b, c, d, e, f] = *outer;
    let [ia, ib, ic, id, ie, iff] = *inner;
    [
        a * ia + c * ib,
        b * ia + d * ib,
        a * ic + c * id,
        b * ic + d * id,
        a * ie + c * iff + e,
        b * ie + d * iff + f,
    ]
}

fn translate(x: f32, y: f32) -> Xform {
    [1.0, 0.0, 0.0, 1.0, x, y]
}

/// Whether `c` stands upright in vertical text: Japanese, Chinese and Korean characters, their punctuation and
/// symbols, and full-width forms. Everything else is set sideways.
pub(crate) fn upright(c: char) -> bool {
    matches!(u32::from(c),
        0x1100..=0x11FF       // Hangul Jamo
        | 0x2E80..=0x2FFF     // CJK radicals, Kangxi radicals, ideographic description
        | 0x3000..=0x303F     // CJK symbols and punctuation (the ideographic space, 、。「」)
        | 0x3040..=0x30FF     // Hiragana, Katakana
        | 0x3100..=0x31FF     // Bopomofo, Hangul compatibility, Kanbun, Katakana extensions
        | 0x3200..=0x33FF     // enclosed CJK, CJK compatibility (㍿)
        | 0x3400..=0x4DBF     // CJK extension A
        | 0x4E00..=0x9FFF     // CJK unified ideographs
        | 0xA960..=0xA97F     // Hangul Jamo extended A
        | 0xAC00..=0xD7FF     // Hangul syllables and Jamo extended B
        | 0xF900..=0xFAFF     // CJK compatibility ideographs
        | 0xFE10..=0xFE1F     // vertical forms
        | 0xFE30..=0xFE4F     // CJK compatibility forms
        | 0xFF00..=0xFFEF     // full-width and half-width forms
        | 0x1F000..=0x1FAFF   // emoji and pictographs
        | 0x20000..=0x3FFFF   // CJK extensions B and later
    )
}

/// `line` cut into runs of upright and of sideways characters, in order.
pub(crate) fn runs(line: &str) -> Vec<(bool, &str)> {
    let mut out: Vec<(bool, &str)> = Vec::new();
    let mut start = 0;
    let mut current: Option<bool> = None;
    for (at, c) in line.char_indices() {
        let up = upright(c);
        if current.is_some_and(|kind| kind != up) {
            if let (Some(kind), Some(run)) = (current, line.get(start..at)) {
                out.push((kind, run));
            }
            start = at;
        }
        current = Some(up);
    }
    if let (Some(kind), Some(run)) = (current, line.get(start..))
        && !run.is_empty()
    {
        out.push((kind, run));
    }
    out
}

/// Lays out `text` vertically at `size` in `family` (and `face`) with `format`: the pieces in the block's own
/// coordinates and the block's bounds `[x0, y0, x1, y1]` (the first column's middle at x 0, the top at y 0).
/// Columns are the format's line pitch apart (自動: `size` × 1.2); `format.align` places each column at the top
/// (left), the middle (centre) or the bottom (right) of the tallest one.
pub(crate) fn vertical_block(
    text: &str,
    family: &str,
    size: f32,
    face: Option<&str>,
    format: &TextFormat,
) -> (Vec<Piece>, [f32; 4]) {
    vertical_block_with(text, family, size, face, format, &[])
}

/// [`vertical_block`] with `room` put after some characters (byte ranges of `text`, 1/1000 em, as tracking is): a
/// justified paragraph text's columns.
pub(crate) fn vertical_block_with(
    text: &str,
    family: &str,
    size: f32,
    face: Option<&str>,
    format: &TextFormat,
    room: &[(std::ops::Range<usize>, f32)],
) -> (Vec<Piece>, [f32; 4]) {
    let column = format.line_pitch_at(size);
    let mut columns: Vec<(Vec<Piece>, f32)> = Vec::new();
    let mut line_start = 0_usize;
    for (index, line) in text.split('\n').enumerate() {
        let mut run_start = line_start;
        line_start = line_start.saturating_add(line.len()).saturating_add(1);
        let line = line.strip_suffix('\r').unwrap_or(line);
        // `as` is exact for the line counts a text holds.
        let cx = -(index as f32) * column;
        let mut pieces = Vec::new();
        let mut cursor = 0.0_f32;
        for (up, run) in runs(line) {
            // The room of this run's characters, from its start.
            let from = run_start;
            run_start = run_start.saturating_add(run.len());
            let mine: Vec<(std::ops::Range<usize>, f32)> = room
                .iter()
                .filter(|(range, _)| range.start >= from && range.end <= run_start)
                .map(|(range, more)| (range.start - from..range.end - from, *more))
                .collect();
            let laid = if mine.is_empty() {
                formatted_layout_in(run, family, size, face, format, up)
            } else if up {
                // A column's layout keeps one tracking for all its characters: the room moves the characters after
                // it down instead.
                let em = crate::draw::room_em(size, format);
                spread_down(
                    &formatted_layout_in(run, family, size, face, format, true),
                    &mine
                        .iter()
                        .map(|(range, more)| (range.end, more * em / 1000.0))
                        .collect::<Vec<_>>(),
                )
            } else {
                crate::draw::formatted_layout_with(run, family, size, face, format, (false, &mine))
            };
            let [x0, y0, x1, y1] = laid.bounds;
            if up {
                // Already a column (centred on x 0): from the cursor down.
                pieces.push((laid, translate(cx, cursor - y0)));
                cursor += (y1 - y0).max(0.0);
            } else {
                // A line turned a quarter clockwise ((x, y) to (-y, x)): its middle on the column, its start at
                // the cursor.
                pieces.push((
                    laid,
                    [0.0, 1.0, -1.0, 0.0, cx + (y0 + y1) * 0.5, cursor - x0],
                ));
                cursor += (x1 - x0).max(0.0);
            }
        }
        columns.push((pieces, cursor));
    }
    let tallest = columns
        .iter()
        .map(|(_, height)| *height)
        .fold(size * 0.5, f32::max);
    let mut pieces = Vec::new();
    for (column_pieces, height) in columns.iter() {
        let shift = match format.align {
            // A justified text's columns are spread down to the box (its last ones start at the top).
            TextAlign::Left | TextAlign::Justify => 0.0,
            TextAlign::Center => (tallest - height) * 0.5,
            TextAlign::Right => tallest - height,
        };
        pieces.extend(
            column_pieces
                .iter()
                .map(|(laid, xf)| (Arc::clone(laid), compose(&translate(0.0, shift), xf))),
        );
    }
    // `as` is exact for the column counts a text holds.
    let count = columns.len().max(1) as f32;
    let bounds = [
        -(count - 1.0) * column - column * 0.5,
        0.0,
        column * 0.5,
        tallest,
    ];
    (pieces, bounds)
}

/// A column's layout with room put in it: each `(at, room)` moves the characters from byte `at` on down by `room`
/// (layout units), and the column grows by all of it.
fn spread_down(laid: &Arc<Layout>, room: &[(usize, f32)]) -> Arc<Layout> {
    let below = |at: usize| -> f32 {
        room.iter()
            .filter(|(from, _)| *from <= at)
            .map(|(_, more)| *more)
            .sum()
    };
    let mut owned = (**laid).clone();
    for glyph in &mut owned.glyphs {
        glyph.y += below(glyph.cluster);
    }
    for line in &mut owned.lines {
        line.baseline += below(line.range.start);
    }
    let total: f32 = room
        .iter()
        .filter(|(from, _)| *from < laid.text_len.max(1))
        .map(|(_, more)| *more)
        .sum();
    owned.bounds[3] += total;
    Arc::new(owned)
}

/// The pieces rasterised into one coverage mask, with where its corner lands. Where pieces overlap the larger
/// coverage counts.
pub(crate) fn rasterize_pieces(pieces: &[Piece]) -> (Mask, i32, i32) {
    let mut rendered: Vec<(Mask, i32, i32)> = pieces
        .iter()
        .map(|(laid, xf)| render::rasterize(laid, xf))
        .filter(|(mask, _, _)| mask.w > 0 && mask.h > 0)
        .collect();
    if rendered.len() <= 1 {
        return rendered.pop().unwrap_or((Mask::new(0, 0), 0, 0));
    }
    let x0 = rendered.iter().map(|(_, x, _)| *x).min().unwrap_or(0);
    let y0 = rendered.iter().map(|(_, _, y)| *y).min().unwrap_or(0);
    let x1 = rendered
        .iter()
        .map(|(mask, x, _)| i64::from(*x) + i64::try_from(mask.w).unwrap_or(0))
        .max()
        .unwrap_or(0);
    let y1 = rendered
        .iter()
        .map(|(mask, _, y)| i64::from(*y) + i64::try_from(mask.h).unwrap_or(0))
        .max()
        .unwrap_or(0);
    let w = usize::try_from(x1 - i64::from(x0)).unwrap_or(0);
    let h = usize::try_from(y1 - i64::from(y0)).unwrap_or(0);
    if w == 0 || h == 0 || w.saturating_mul(h) > MAX_MASK_PIXELS {
        return (Mask::new(0, 0), 0, 0);
    }
    let mut out = Mask::new(w, h);
    for (mask, x, y) in &rendered {
        let dx = usize::try_from(i64::from(*x) - i64::from(x0)).unwrap_or(0);
        let dy = usize::try_from(i64::from(*y) - i64::from(y0)).unwrap_or(0);
        for row in 0..mask.h {
            for col in 0..mask.w {
                let value = mask.a.get(row * mask.w + col).copied().unwrap_or(0.0);
                if value <= 0.0 {
                    continue;
                }
                if let Some(slot) = out.a.get_mut((dy + row) * w + dx + col) {
                    *slot = slot.max(value);
                }
            }
        }
    }
    (out, x0, y0)
}

#[cfg(test)]
mod tests {
    use super::{IDENTITY, compose, runs, upright};

    /// Japanese stands upright; Latin letters, digits and half-width punctuation go sideways; a line splits where
    /// that changes.
    #[test]
    fn runs_split_where_the_writing_turns() {
        assert!(upright('あ') && upright('ア') && upright('漢') && upright('、') && upright('Ａ'));
        assert!(!upright('A') && !upright('1') && !upright(' ') && !upright('-'));
        assert_eq!(
            runs("第2章 DVD版です"),
            vec![
                (true, "第"),
                (false, "2"),
                (true, "章"),
                (false, " DVD"),
                (true, "版です")
            ]
        );
        assert!(runs("").is_empty());
    }

    /// Composing transforms applies the inner one first.
    #[test]
    fn transforms_compose_inner_first() {
        let turn = [0.0, 1.0, -1.0, 0.0, 0.0, 0.0];
        let shift = [1.0, 0.0, 0.0, 1.0, 10.0, 0.0];
        // Turn (1, 1) to (-1, 1), then shift to (9, 1).
        let [a, b, c, d, e, f] = compose(&shift, &turn);
        assert_eq!((a + c + e, b + d + f), (9.0, 1.0));
        assert_eq!(compose(&IDENTITY, &shift), shift);
    }
}
