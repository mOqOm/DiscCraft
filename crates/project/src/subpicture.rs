//! A button's own subpicture (Encore's `(=1)` / `(=2)` / `(=3)` layers of a PSD button).

use serde::{Deserialize, Serialize};

/// A button's own subpicture: its highlight instead of a frame or its text, each pixel colour 1 to 3 of the menu's
/// colour set (0 none). It is stretched over the button's content box (or its own [`Subpicture::area`] of it), so it
/// follows the button when it is moved or resized.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subpicture {
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
    /// The pixels row by row: each row a list of runs, a letter (`a` to `d` for colour 0 to 3) and how many pixels
    /// (left out for one), ending with `/`. A row's missing pixels at the end are 0.
    pub runs: String,
    /// Where it lies relative to the button's content box, when it reaches past it (Encore's Paste as Subpicture of
    /// a layer that sticks out of the button, which widens the button, M6.329); `None`: over the content box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<crate::menu::ContentBox>,
    /// The PSD button's `(=1)`, `(=2)` and `(=3)` layers in their own colours, the same size as the subpicture, for
    /// a Blu-ray menu with rich-colour highlights (M6.335; see [`Subpicture::color_states`]). None for a
    /// subpicture made in DiscCraft.
    #[serde(default, skip_serializing_if = "no_color_layers")]
    pub color_layers: [Option<ColorLayer>; 3],
}

fn no_color_layers(layers: &[Option<ColorLayer>; 3]) -> bool {
    layers.iter().all(Option::is_none)
}

/// The longest side of a subpicture (a PSD's largest canvas).
pub const MAX_SUBPICTURE_SIDE: u16 = 8192;

/// The most colours a [`ColorLayer`] keeps (a Blu-ray highlight page has 255 palette entries).
pub const MAX_LAYER_COLORS: usize = 255;

/// One of a PSD button's subpicture layers in its own colours (Encore's `(=1)` / `(=2)` / `(=3)` layer as a Blu-ray
/// menu with rich-colour highlights shows it, M6.335), the size of its [`Subpicture`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorLayer {
    /// Its colours, straight-alpha RGBA; pixel value `n` is colour `n` (counted from 1), 0 none.
    pub colors: Vec<[u8; 4]>,
    /// The pixels row by row: runs of a value, `value` or `value*count`, separated by `,`, each row ending with `/`.
    /// A row's missing pixels at the end are 0.
    pub runs: String,
}

impl ColorLayer {
    /// A layer of `width` x `height` straight-alpha RGBA pixels (`width * height * 4` bytes): more than
    /// [`MAX_LAYER_COLORS`] colours are brought down to that many. `None` when the sizes are out of range or do not
    /// match, or nothing is drawn.
    pub fn from_rgba(width: u16, height: u16, rgba: &[u8]) -> Option<Self> {
        if width == 0
            || height == 0
            || width > MAX_SUBPICTURE_SIDE
            || height > MAX_SUBPICTURE_SIDE
            || rgba.len()
                != usize::from(width)
                    .checked_mul(usize::from(height))?
                    .checked_mul(4)?
        {
            return None;
        }
        let pixels = rgba.as_chunks::<4>().0;
        let mut counts: std::collections::HashMap<[u8; 4], u64> = std::collections::HashMap::new();
        for pixel in pixels.iter().filter(|pixel| pixel[3] > 0) {
            *counts.entry(*pixel).or_default() += 1;
        }
        if counts.is_empty() {
            return None;
        }
        let mut found: Vec<([u8; 4], u64)> = counts.into_iter().collect();
        // Most used first, so a layer's runs stay short and the order does not depend on the map.
        found.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let (colors, mapping) = crate::quantize::reduce_colors(&found, MAX_LAYER_COLORS);
        let value_of: std::collections::HashMap<[u8; 4], u8> = found
            .iter()
            .zip(&mapping)
            .filter_map(|((color, _), index)| Some((*color, u8::try_from(index + 1).ok()?)))
            .collect();
        let mut runs = String::new();
        for row in pixels.chunks(usize::from(width)) {
            let values: Vec<u8> = row
                .iter()
                .map(|pixel| {
                    if pixel[3] == 0 {
                        0
                    } else {
                        value_of.get(pixel).copied().unwrap_or(0)
                    }
                })
                .collect();
            // Trailing empty pixels are left out.
            let used = values
                .iter()
                .rposition(|value| *value != 0)
                .map_or(0, |last| last + 1);
            let mut values = values.iter().take(used).peekable();
            let mut first = true;
            while let Some(&value) = values.next() {
                let mut count = 1_usize;
                while values.next_if_eq(&&value).is_some() {
                    count += 1;
                }
                if !first {
                    runs.push(',');
                }
                first = false;
                runs.push_str(&value.to_string());
                if count > 1 {
                    runs.push('*');
                    runs.push_str(&count.to_string());
                }
            }
            runs.push('/');
        }
        Some(Self { colors, runs })
    }

    /// Its pixels as straight-alpha RGBA (`width * height * 4` bytes) for a subpicture of `width` x `height`;
    /// `None` when the runs are not valid for the size. A value without a colour is clear.
    pub fn rgba(&self, width: u16, height: u16) -> Option<Vec<u8>> {
        if width == 0 || height == 0 || width > MAX_SUBPICTURE_SIDE || height > MAX_SUBPICTURE_SIDE
        {
            return None;
        }
        let row_bytes = usize::from(width).checked_mul(4)?;
        let mut rgba = Vec::with_capacity(row_bytes.checked_mul(usize::from(height))?);
        let mut rows = 0_u16;
        for row in self.runs.split_terminator('/') {
            rows = rows.checked_add(1).filter(|rows| *rows <= height)?;
            let start = rgba.len();
            for run in row.split(',').filter(|run| !run.is_empty()) {
                let (value, count) = match run.split_once('*') {
                    Some((value, count)) => (value, count.parse::<usize>().ok()?),
                    None => (run, 1),
                };
                let value = value.parse::<u8>().ok()?;
                let added = count.checked_mul(4)?;
                if count == 0 || (rgba.len() - start).checked_add(added)? > row_bytes {
                    return None;
                }
                let color = usize::from(value)
                    .checked_sub(1)
                    .and_then(|index| self.colors.get(index))
                    .copied()
                    .unwrap_or([0; 4]);
                for _ in 0..count {
                    rgba.extend_from_slice(&color);
                }
            }
            rgba.resize(start + row_bytes, 0);
        }
        (rows == height).then_some(rgba)
    }
}

impl Subpicture {
    /// The colour layers it shows in the normal, selected and activated states as a rich-colour Blu-ray highlight,
    /// as Encore shows them: a subpicture with only a `(=1)` layer shows it when selected and when activated (and
    /// nothing in the normal state); with a `(=2)` or `(=3)` layer each state shows its own — `(=1)` normal, `(=2)`
    /// selected, `(=3)` activated. `None` without colour layers.
    pub fn color_states(&self) -> Option<[Option<&ColorLayer>; 3]> {
        let [first, second, third] = &self.color_layers;
        match (first, second, third) {
            (None, None, None) => None,
            (Some(only), None, None) => Some([None, Some(only), Some(only)]),
            _ => Some([first.as_ref(), second.as_ref(), third.as_ref()]),
        }
    }
    /// A subpicture of `width` x `height` pixels (values 0 to 3, row by row); `None` when the sizes are out of range
    /// or do not match, a value is above 3, or nothing is drawn.
    pub fn from_values(width: u16, height: u16, values: &[u8]) -> Option<Self> {
        if width == 0
            || height == 0
            || width > MAX_SUBPICTURE_SIDE
            || height > MAX_SUBPICTURE_SIDE
            || values.len() != usize::from(width).checked_mul(usize::from(height))?
            || values.iter().any(|value| *value > 3)
            || values.iter().all(|value| *value == 0)
        {
            return None;
        }
        let mut runs = String::new();
        for row in values.chunks(usize::from(width)) {
            // Trailing empty pixels are left out.
            let used = row
                .iter()
                .rposition(|value| *value != 0)
                .map_or(0, |last| last + 1);
            let mut pixels = row.iter().take(used).peekable();
            while let Some(&value) = pixels.next() {
                let mut count = 1_usize;
                while pixels.next_if_eq(&&value).is_some() {
                    count += 1;
                }
                runs.push(char::from(b'a' + value));
                if count > 1 {
                    runs.push_str(&count.to_string());
                }
            }
            runs.push('/');
        }
        Some(Self {
            width,
            height,
            runs,
            area: None,
            color_layers: Default::default(),
        })
    }

    /// The frame rectangle it is drawn over for a button whose content box is `content`: its area placed by the
    /// box, or the box itself.
    pub fn rect_in(&self, content: crate::Rect) -> crate::Rect {
        let Some(area) = self.area else {
            return content;
        };
        let place = |start: u16, offset: i16| {
            u16::try_from((i32::from(start) + i32::from(offset)).max(0)).unwrap_or(u16::MAX)
        };
        crate::Rect {
            x: place(content.x, area.x),
            y: place(content.y, area.y),
            w: area.w.max(1),
            h: area.h.max(1),
        }
    }

    /// The pixels (values 0 to 3, row by row); `None` when the runs are not valid for the size.
    pub fn values(&self) -> Option<Vec<u8>> {
        if self.width == 0
            || self.height == 0
            || self.width > MAX_SUBPICTURE_SIDE
            || self.height > MAX_SUBPICTURE_SIDE
        {
            return None;
        }
        let width = usize::from(self.width);
        let mut values = Vec::with_capacity(width.checked_mul(usize::from(self.height))?);
        let mut rows = 0_u16;
        for row in self.runs.split_terminator('/') {
            rows = rows.checked_add(1).filter(|rows| *rows <= self.height)?;
            let start = values.len();
            let mut chars = row.chars().peekable();
            while let Some(letter) = chars.next() {
                let value = match letter {
                    'a'..='d' => u8::try_from(u32::from(letter) - u32::from('a')).ok()?,
                    _ => return None,
                };
                let mut count: Option<usize> = None;
                while let Some(digit) = chars.next_if(char::is_ascii_digit) {
                    let digit = usize::try_from(digit.to_digit(10)?).ok()?;
                    count = Some(count.unwrap_or(0).checked_mul(10)?.checked_add(digit)?);
                }
                let count = count.unwrap_or(1);
                if count == 0 || (values.len() - start).checked_add(count)? > width {
                    return None;
                }
                values.extend(std::iter::repeat_n(value, count));
            }
            values.resize(start + width, 0);
        }
        if rows != self.height {
            return None;
        }
        Some(values)
    }
}

#[cfg(test)]
mod tests {
    use super::{ColorLayer, Subpicture};

    #[test]
    fn values_go_to_runs_and_back() {
        let values = [
            0, 1, 1, 1, 0, 0, //
            0, 0, 0, 0, 0, 0, //
            2, 2, 3, 3, 3, 3,
        ];
        let sub = Subpicture::from_values(6, 3, &values);
        assert_eq!(
            sub.as_ref().map(|sub| sub.runs.as_str()),
            Some("ab3//c2d4/")
        );
        assert_eq!(
            sub.and_then(|sub| sub.values()).as_deref(),
            Some(&values[..])
        );
    }

    #[test]
    fn bad_input_gives_none() {
        assert!(Subpicture::from_values(2, 1, &[0, 0]).is_none());
        assert!(Subpicture::from_values(2, 1, &[4, 0]).is_none());
        assert!(Subpicture::from_values(2, 2, &[1, 0]).is_none());
        assert!(Subpicture::from_values(0, 1, &[]).is_none());
        for runs in [
            "b3/",
            "b/",
            "b/b/b/",
            "e/",
            "b0/",
            "b99999999999999999999/",
            "x",
        ] {
            let sub = Subpicture {
                width: 2,
                height: 2,
                runs: runs.to_owned(),
                area: None,
                color_layers: Default::default(),
            };
            assert!(sub.values().is_none(), "{runs}");
        }
        let fine = Subpicture {
            width: 2,
            height: 2,
            runs: "b/c2/".to_owned(),
            area: None,
            color_layers: Default::default(),
        };
        assert_eq!(fine.values(), Some(vec![1, 0, 2, 2]));
    }

    /// M6.335: a layer in its own colours goes to runs and back, half-clear pixels and all.
    #[test]
    fn a_colour_layer_keeps_its_colours() -> Result<(), serde_json::Error> {
        let red = [255, 0, 0, 255];
        let soft = [255, 220, 0, 100];
        let clear = [0, 0, 0, 0];
        let rgba: Vec<u8> = [
            red, red, red, soft, clear, clear, clear, clear, soft, red, clear, clear,
        ]
        .concat();
        let layer = ColorLayer::from_rgba(4, 3, &rgba);
        assert_eq!(
            layer.as_ref().map(|layer| layer.runs.as_str()),
            Some("1*3,2//2,1/")
        );
        assert_eq!(
            layer.as_ref().map(|layer| layer.colors.clone()),
            Some(vec![red, soft])
        );
        assert_eq!(
            layer.as_ref().and_then(|layer| layer.rgba(4, 3)),
            Some(rgba)
        );
        // Through the project file.
        let text = serde_json::to_string(&layer)?;
        let back: Option<ColorLayer> = serde_json::from_str(&text)?;
        assert_eq!(back, layer);
        // Nothing drawn, wrong sizes, bad runs.
        assert!(ColorLayer::from_rgba(1, 1, &clear).is_none());
        assert!(ColorLayer::from_rgba(2, 1, &red).is_none());
        for runs in ["1*5/", "x/", "1*0/", "1/1/1/1/", "300/"] {
            let bad = ColorLayer {
                colors: vec![red],
                runs: runs.to_owned(),
            };
            assert!(bad.rgba(4, 3).is_none(), "{runs}");
        }
        Ok(())
    }

    /// M6.335: which colour layer each state shows (Encore's rich-colour highlights).
    #[test]
    fn a_lone_first_layer_is_the_highlight_and_more_layers_are_the_states() -> Result<(), String> {
        let layer = |red: u8| ColorLayer::from_rgba(1, 1, &[red, 0, 0, 255]);
        let mut sub = Subpicture::from_values(1, 1, &[1]).ok_or("subpicture")?;
        assert!(sub.color_states().is_none());
        sub.color_layers = [layer(1), None, None];
        let reds = |sub: &Subpicture| {
            sub.color_states()
                .map(|states| states.map(|state| state.map(|layer| layer.colors.clone())))
        };
        let one = Some(vec![[1, 0, 0, 255]]);
        assert_eq!(reds(&sub), Some([None, one.clone(), one.clone()]));
        sub.color_layers = [layer(1), None, layer(3)];
        assert_eq!(reds(&sub), Some([one, None, Some(vec![[3, 0, 0, 255]])]));
        sub.color_layers = [None, layer(2), None];
        assert_eq!(reds(&sub), Some([None, Some(vec![[2, 0, 0, 255]]), None]));
        Ok(())
    }
}
