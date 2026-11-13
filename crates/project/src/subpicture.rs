//! A button's own subpicture (Encore's `(=1)` / `(=2)` / `(=3)` layers of a PSD button).

use serde::{Deserialize, Serialize};

/// A button's own subpicture: its highlight instead of a frame or its text, each pixel colour 1 to 3 of the menu's
/// colour set (0 none). It is stretched over the button, so it follows the button when it is moved or resized.
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
}

/// The longest side of a subpicture (a PSD's largest canvas).
pub const MAX_SUBPICTURE_SIDE: u16 = 8192;

impl Subpicture {
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
        })
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
    use super::Subpicture;

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
            };
            assert!(sub.values().is_none(), "{runs}");
        }
        let fine = Subpicture {
            width: 2,
            height: 2,
            runs: "b/c2/".to_owned(),
        };
        assert_eq!(fine.values(), Some(vec![1, 0, 2, 2]));
    }
}
