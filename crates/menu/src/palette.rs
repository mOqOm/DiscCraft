//! Menu palette helpers.

use crate::HighlightLayer;
use disccraft_project::{Menu, opacity_to_contrast};
use std::collections::VecDeque;

/// Packs RGB into a BT.601 studio-range YCrCb entry.
///
/// The returned value is packed as `0x00_YY_CR_CB`.
pub fn ycrcb_entry(rgb: [u8; 3]) -> u32 {
    let [r, g, b] = rgb;

    let rf = f64::from(r);
    let gf = f64::from(g);
    let bf = f64::from(b);

    let y = 16.0 + (219.0 / 255.0) * (0.299 * rf + 0.587 * gf + 0.114 * bf);
    let cb = 128.0 + (224.0 / 255.0) * (-0.168736 * rf - 0.331264 * gf + 0.5 * bf);
    let cr = 128.0 + (224.0 / 255.0) * (0.5 * rf - 0.418688 * gf - 0.081312 * bf);

    let y = to_u8(y);
    let cb = to_u8(cb);
    let cr = to_u8(cr);

    (u32::from(y) << 16) | (u32::from(cr) << 8) | u32::from(cb)
}

/// Palette entries of a highlight group's selected and activated colours (group 1: 1 and 2, group 2: 4 and 5,
/// group 3: 6 and 7; 0 is black and 3 the text colour).
pub(crate) const GROUP_ENTRIES: [(usize, usize); 3] = [(1, 2), (4, 5), (6, 7)];

/// Palette entry of the normal colour (Encore's 標準グループ): the highlight frame of a button that is neither
/// selected nor activated.
pub const NORMAL_ENTRY: u8 = 8;

/// The 16-entry CLUT: 0 black, 3 the text colour, each highlight group's selected / activated colour, and the
/// normal colour (entry 8).
pub(crate) fn build(menu: &Menu) -> [u32; 16] {
    let mut palette = [0u32; 16];

    if let Some(slot) = palette.get_mut(0) {
        *slot = ycrcb_entry([0, 0, 0]);
    }
    if let Some(slot) = palette.get_mut(3) {
        *slot = ycrcb_entry(menu.text_color);
    }
    let set = menu.color_set_or_default();
    for (group, (selected, activated)) in set.groups.iter().zip(GROUP_ENTRIES) {
        if let Some(slot) = palette.get_mut(selected) {
            *slot = ycrcb_entry(group.selected);
        }
        if let Some(slot) = palette.get_mut(activated) {
            *slot = ycrcb_entry(group.activated_look().0);
        }
    }
    if let Some(slot) = palette.get_mut(usize::from(NORMAL_ENTRY)) {
        *slot = ycrcb_entry(set.normal.color);
    }

    palette
}

/// The DVD colours of a menu's highlight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DvdColors {
    /// The 16-entry CLUT.
    pub(crate) palette: [u32; 16],
    /// The SL_COLI of highlight groups 1 to 3.
    pub(crate) color_sets: [[u8; 8]; 3],
    /// The palette entry of each pixel value (0 to 3) in the normal state.
    pub(crate) normal_entries: [u8; 4],
    /// The contrast (0 to 15) of each pixel value in the normal state; 0 hides it.
    pub(crate) normal_contrasts: [u8; 4],
}

/// The DVD colours of `menu` whose highlight is `layer`. Colour 1 (pixel value 1, the pattern pixels) has the fixed
/// entries of [`build`]; colours 2 and 3 (values 2 and 3, the emphasis pixels 1 and 2 of a subpicture's `(=2)` /
/// `(=3)` layers), when the layer has them, take the entries left (9 to 15, then those of the highlight groups no
/// button uses), sharing an entry with the same colour, or the nearest colour once the 16 entries run out. An unseen
/// colour (opacity 0) takes no entry.
pub(crate) fn dvd_colors(menu: &Menu, layer: &HighlightLayer) -> DvdColors {
    let set = menu.color_set_or_default();
    let mut colors = DvdColors {
        palette: build(menu),
        color_sets: color_sets(menu),
        normal_entries: [0, NORMAL_ENTRY, 3, 3],
        normal_contrasts: [0, opacity_to_contrast(set.normal.opacity), 0, 0],
    };
    let mut used_groups = [false; 3];
    for button in &menu.buttons {
        if let Some(used) = used_groups.get_mut(usize::from(button.highlight_group.clamp(1, 3) - 1))
        {
            *used = true;
        }
    }
    let mut entries = Entries::new(used_groups);
    for n in [2_u8, 3] {
        if !layer.pixels.contains(&n) {
            continue;
        }
        for (index, group) in set.groups.iter().enumerate() {
            if !used_groups.get(index).copied().unwrap_or(false) {
                continue;
            }
            let Some(sl_coli) = colors.color_sets.get_mut(index) else {
                continue;
            };
            let (selection, action) = sl_coli.split_at_mut(4);
            for (half, (color, opacity)) in [
                (selection, group.selected_color(n)),
                (action, group.activated_color(n)),
            ] {
                let contrast = opacity_to_contrast(opacity);
                let entry = entries.take(&mut colors.palette, color, contrast);
                set_emphasis(half, n, entry, contrast);
            }
        }
        let normal = set.normal_color(n);
        let contrast = opacity_to_contrast(normal.opacity);
        let entry = entries.take(&mut colors.palette, normal.color, contrast);
        if let (Some(slot), Some(alpha)) = (
            colors.normal_entries.get_mut(usize::from(n)),
            colors.normal_contrasts.get_mut(usize::from(n)),
        ) {
            *slot = entry;
            *alpha = contrast;
        }
    }
    colors
}

/// Point the emphasis pixels of colour `n` (2: emphasis 1, the low nibbles; 3: emphasis 2, the high nibbles) of one
/// half of an SL_COLI (colours in byte 0, contrasts in byte 2) at `entry` with `contrast`.
fn set_emphasis(half: &mut [u8], n: u8, entry: u8, contrast: u8) {
    let (shift, keep) = if n == 3 { (4, 0x0F) } else { (0, 0xF0) };
    if let Some(byte) = half.get_mut(0) {
        *byte = (*byte & keep) | ((entry & 0x0F) << shift);
    }
    if let Some(byte) = half.get_mut(2) {
        *byte = (*byte & keep) | ((contrast & 0x0F) << shift);
    }
}

/// The CLUT entries colours 2 and 3 can use.
struct Entries {
    /// Entries holding a colour that is shown.
    used: Vec<usize>,
    /// Entries still free.
    free: VecDeque<usize>,
}

impl Entries {
    fn new(used_groups: [bool; 3]) -> Self {
        let mut used = vec![0, 3, usize::from(NORMAL_ENTRY)];
        let mut free: VecDeque<usize> = (usize::from(NORMAL_ENTRY) + 1..16).collect();
        for ((selected, activated), in_use) in GROUP_ENTRIES.into_iter().zip(used_groups) {
            if in_use {
                used.extend([selected, activated]);
            } else {
                free.extend([selected, activated]);
            }
        }
        Self { used, free }
    }

    /// The entry for `color`: one already holding it, a free one, or the nearest. Entry 0 when it is not seen.
    fn take(&mut self, palette: &mut [u32; 16], color: [u8; 3], contrast: u8) -> u8 {
        if contrast == 0 {
            return 0;
        }
        let value = ycrcb_entry(color);
        let entry = if let Some(&entry) = self
            .used
            .iter()
            .find(|&&entry| palette.get(entry) == Some(&value))
        {
            entry
        } else if let Some(entry) = self.free.pop_front() {
            if let Some(slot) = palette.get_mut(entry) {
                *slot = value;
            }
            self.used.push(entry);
            entry
        } else {
            self.used
                .iter()
                .copied()
                .min_by_key(|&entry| distance(palette.get(entry).copied().unwrap_or(0), value))
                .unwrap_or(0)
        };
        // Entries are below 16.
        u8::try_from(entry).unwrap_or(0)
    }
}

/// The squared distance of two packed YCrCb entries.
fn distance(a: u32, b: u32) -> u32 {
    [16_u32, 8, 0]
        .iter()
        .map(|shift| {
            let d = ((a >> shift) & 0xFF).abs_diff((b >> shift) & 0xFF);
            d * d
        })
        .sum()
}

/// The three SL_COLI entries: selection colour / contrast and action colour / contrast, each pointing at the
/// group's palette entries with the group's opacities (emphasis pixels keep the text colour, entry 3).
pub(crate) fn color_sets(menu: &Menu) -> [[u8; 8]; 3] {
    let colors = menu.color_set_or_default();
    let mut sets = [[0u8; 8]; 3];
    for ((set, (selected, activated)), group) in
        sets.iter_mut().zip(GROUP_ENTRIES).zip(colors.groups)
    {
        // `as` is exact: the entries are below 16.
        let selected = (selected as u8) << 4;
        let activated = (activated as u8) << 4;
        let selected_contrast = opacity_to_contrast(group.selected_opacity) << 4;
        let activated_contrast = opacity_to_contrast(group.activated_look().1) << 4;
        *set = [
            0x33,
            selected,
            0x00,
            selected_contrast,
            0x33,
            activated,
            0x00,
            activated_contrast,
        ];
    }
    sets
}

fn to_u8(value: f64) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::{NORMAL_ENTRY, build, color_sets, dvd_colors, ycrcb_entry};
    use crate::HighlightLayer;
    use disccraft_project::{ColorSet, GroupColor, HighlightGroup, Menu, NormalColor, Project};

    fn plain_menu() -> Option<Menu> {
        let mut project = Project::default();
        let id = project.add_menu(None).ok()?;
        project.menu(id).cloned()
    }

    fn menu_with(set: ColorSet) -> Option<Menu> {
        let mut menu = plain_menu()?;
        menu.color_set = Some(set);
        Some(menu)
    }

    #[test]
    fn opacities_set_the_contrasts_and_the_check_box_the_activated_entry()
    -> Result<(), &'static str> {
        let mut group = HighlightGroup::new([255, 0, 0], [0, 0, 255]);
        group.selected_opacity = 47;
        group.activated_opacity = 0;
        let mut same = group;
        same.same_activated = true;
        let menu = menu_with(ColorSet {
            groups: [group, same, HighlightGroup::new([1, 1, 1], [2, 2, 2])],
            normal: NormalColor {
                color: [0, 255, 0],
                opacity: 100,
            },
            normal_more: None,
        })
        .ok_or("menu")?;
        let sets = color_sets(&menu);
        assert_eq!(sets[0], [0x33, 0x10, 0x00, 0x70, 0x33, 0x20, 0x00, 0x00]);
        // "Same as selected": the activated contrast is the selected one, and its entry holds the selected colour.
        assert_eq!(sets[1][7], 0x70);
        let palette = build(&menu);
        assert_eq!(palette[5], ycrcb_entry([255, 0, 0]));
        assert_eq!(palette[usize::from(NORMAL_ENTRY)], ycrcb_entry([0, 255, 0]));
        let layer = layer(&[0, 1]);
        assert_eq!(dvd_colors(&menu, &layer).normal_contrasts, [0, 15, 0, 0]);
        let plain = plain_menu().ok_or("menu")?;
        assert_eq!(dvd_colors(&plain, &layer).normal_contrasts, [0, 0, 0, 0]);
        Ok(())
    }

    fn layer(pixels: &[u8]) -> HighlightLayer {
        HighlightLayer {
            x: 0,
            y: 0,
            width: u16::try_from(pixels.len()).unwrap_or(1),
            height: 1,
            pixels: pixels.to_vec(),
            coverage: pixels
                .iter()
                .map(|&p| if p > 0 { 255 } else { 0 })
                .collect(),
        }
    }

    #[test]
    fn colours_two_and_three_take_the_free_entries() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        let id = project.add_menu(None)?;
        project.add_button(id, "A".to_owned(), None, disccraft_project::Link::Stop)?;
        let mut menu = project.menu(id).cloned().ok_or("menu")?;
        let mut group = HighlightGroup::new([255, 0, 0], [0, 0, 255]);
        group.more = Some([
            GroupColor {
                selected: [0, 255, 0],
                selected_opacity: 100,
                activated: [0, 0, 255],
                activated_opacity: 0,
            },
            GroupColor {
                selected: [10, 20, 30],
                selected_opacity: 47,
                activated: [255, 0, 0],
                activated_opacity: 100,
            },
        ]);
        menu.color_set = Some(ColorSet {
            groups: [group; 3],
            normal: NormalColor::default(),
            normal_more: Some([
                NormalColor {
                    color: [7, 7, 7],
                    opacity: 100,
                },
                NormalColor::default(),
            ]),
        });

        // Without colours 2 and 3 in the highlight, nothing changes.
        let plain = dvd_colors(&menu, &layer(&[0, 1, 1]));
        assert_eq!(plain.palette, build(&menu));
        assert_eq!(plain.color_sets, color_sets(&menu));

        let colors = dvd_colors(&menu, &layer(&[0, 1, 2, 3]));
        let [selection_e, _, selection_c, _, action_e, _, action_c, _] = colors.color_sets[0];
        // Colour 2 first: selected green in a new entry (9), activated unseen (entry 0, contrast 0), normal grey in
        // the next (10). Then colour 3: selected in 11 at 47 % (contrast 7), activated red sharing group 1's
        // selected entry (1), normal unseen.
        assert_eq!(selection_e, 0xB9);
        assert_eq!(selection_c, 0x7F);
        assert_eq!(colors.palette[9], ycrcb_entry([0, 255, 0]));
        assert_eq!(colors.palette[10], ycrcb_entry([7, 7, 7]));
        assert_eq!(colors.palette[11], ycrcb_entry([10, 20, 30]));
        assert_eq!(action_e, 0x10);
        assert_eq!(action_c, 0xF0);
        assert_eq!(colors.normal_entries, [0, NORMAL_ENTRY, 10, 0]);
        assert_eq!(colors.normal_contrasts, [0, 0, 15, 0]);
        // Groups no button uses are left alone.
        assert_eq!(colors.color_sets[1], color_sets(&menu)[1]);
        Ok(())
    }

    #[test]
    fn colours_share_the_nearest_entry_when_none_is_left() {
        let mut entries = super::Entries::new([true; 3]);
        let mut palette = [0_u32; 16];
        for (index, slot) in palette.iter_mut().enumerate() {
            let level = u8::try_from(index * 16).unwrap_or(255);
            *slot = ycrcb_entry([level, level, level]);
        }
        // Seven free entries (9 to 15), then the nearest.
        for step in 0..7_u8 {
            assert_eq!(
                entries.take(&mut palette, [step * 30, 255, 0], 15),
                9 + step
            );
        }
        assert_eq!(entries.take(&mut palette, [92, 255, 0], 15), 12);
        assert_eq!(entries.take(&mut palette, [3, 255, 0], 0), 0);
    }
}
