//! Menu palette helpers.

use disccraft_project::{Menu, opacity_to_contrast};

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

/// The contrast (0 to 15) of the highlight frame in the normal state; 0 hides it.
pub(crate) fn normal_alpha(menu: &Menu) -> u8 {
    opacity_to_contrast(menu.color_set_or_default().normal.opacity)
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
    use super::{NORMAL_ENTRY, build, color_sets, normal_alpha, ycrcb_entry};
    use disccraft_project::{ColorSet, HighlightGroup, Menu, NormalColor, Project};

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
        })
        .ok_or("menu")?;
        let sets = color_sets(&menu);
        assert_eq!(sets[0], [0x33, 0x10, 0x00, 0x70, 0x33, 0x20, 0x00, 0x00]);
        // "Same as selected": the activated contrast is the selected one, and its entry holds the selected colour.
        assert_eq!(sets[1][7], 0x70);
        let palette = build(&menu);
        assert_eq!(palette[5], ycrcb_entry([255, 0, 0]));
        assert_eq!(palette[usize::from(NORMAL_ENTRY)], ycrcb_entry([0, 255, 0]));
        assert_eq!(normal_alpha(&menu), 15);
        assert_eq!(normal_alpha(&plain_menu().ok_or("menu")?), 0);
        Ok(())
    }
}
