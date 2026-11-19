//! Bringing many colours down to a few (median cut): a Blu-ray highlight page has 255 palette entries, and a PSD
//! button's subpicture layers in their own colours (M6.335) can have more.

/// Brings `colors` (each a straight-alpha RGBA colour with how many pixels have it) down to at most `max` colours
/// (at least 1): the palette, and for each colour of `colors` the index of the palette colour it becomes. Colours
/// that already fit are kept as they are, in their order.
///
/// Median cut: the group of colours with the widest channel (red, green, blue or alpha) is split at its
/// pixel-weighted median along that channel, until there are `max` groups; each group becomes its pixel-weighted
/// average.
pub fn reduce_colors(colors: &[([u8; 4], u64)], max: usize) -> (Vec<[u8; 4]>, Vec<usize>) {
    let max = max.max(1);
    if colors.len() <= max {
        return (
            colors.iter().map(|(color, _)| *color).collect(),
            (0..colors.len()).collect(),
        );
    }
    let color = |index: usize| colors.get(index).map_or([0; 4], |(color, _)| *color);
    let weight = |index: usize| colors.get(index).map_or(1, |(_, count)| (*count).max(1));
    let mut groups: Vec<Vec<usize>> = vec![(0..colors.len()).collect()];
    while groups.len() < max {
        // The widest channel of any group that can be split.
        let mut widest: Option<(usize, usize, u8)> = None;
        for (group_index, members) in groups.iter().enumerate() {
            if members.len() < 2 {
                continue;
            }
            for channel in 0..4 {
                let values = members
                    .iter()
                    .map(|index| color(*index).get(channel).copied().unwrap_or(0));
                let low = values.clone().min().unwrap_or(0);
                let high = values.max().unwrap_or(0);
                let range = high - low;
                if widest.is_none_or(|(_, _, best)| range > best) {
                    widest = Some((group_index, channel, range));
                }
            }
        }
        let Some((group_index, channel, _)) = widest.filter(|(_, _, range)| *range > 0) else {
            break;
        };
        let mut members = groups.swap_remove(group_index);
        members.sort_by_key(|index| color(*index).get(channel).copied().unwrap_or(0));
        let total: u64 = members.iter().map(|index| weight(*index)).sum();
        let mut running = 0_u64;
        let mut split = members.len() / 2;
        for (position, index) in members.iter().enumerate() {
            running = running.saturating_add(weight(*index));
            if running.saturating_mul(2) >= total {
                split = position + 1;
                break;
            }
        }
        let split = split.clamp(1, members.len() - 1);
        let upper = members.split_off(split);
        groups.push(members);
        groups.push(upper);
    }
    let mut palette = Vec::with_capacity(groups.len());
    let mut mapping = vec![0_usize; colors.len()];
    for (palette_index, members) in groups.iter().enumerate() {
        let mut sums = [0_u128; 4];
        let mut total = 0_u128;
        for index in members {
            let pixels = u128::from(weight(*index));
            for (sum, value) in sums.iter_mut().zip(color(*index)) {
                *sum += u128::from(value) * pixels;
            }
            total += pixels;
            if let Some(slot) = mapping.get_mut(*index) {
                *slot = palette_index;
            }
        }
        let total = total.max(1);
        // The average of 0..=255 values stays within 0..=255.
        palette.push(sums.map(|sum| u8::try_from((sum + total / 2) / total).unwrap_or(u8::MAX)));
    }
    (palette, mapping)
}

#[cfg(test)]
mod tests {
    use super::reduce_colors;

    #[test]
    fn few_colours_are_kept_and_many_are_brought_down() {
        let few = [([255, 0, 0, 255], 4), ([0, 0, 255, 128], 1)];
        let (palette, mapping) = reduce_colors(&few, 255);
        assert_eq!(palette, vec![[255, 0, 0, 255], [0, 0, 255, 128]]);
        assert_eq!(mapping, vec![0, 1]);

        // A grey ramp of 256 colours into 4: each colour goes to the nearest quarter.
        let ramp: Vec<([u8; 4], u64)> = (0..=255_u8).map(|v| ([v, v, v, 255], 1)).collect();
        let (palette, mapping) = reduce_colors(&ramp, 4);
        assert_eq!(palette.len(), 4);
        assert_eq!(mapping.len(), 256);
        for (value, index) in mapping.iter().enumerate() {
            let grey = palette.get(*index).map_or(0, |colour| colour[0]);
            assert!(
                usize::from(grey).abs_diff(value) <= 32,
                "{value} became {grey}"
            );
        }
        // Many pixels of one colour keep it close.
        let mut weighted = ramp.clone();
        if let Some(entry) = weighted.get_mut(10) {
            entry.1 = 100_000;
        }
        let (palette, mapping) = reduce_colors(&weighted, 2);
        let ten = mapping.get(10).and_then(|index| palette.get(*index));
        assert!(ten.is_some_and(|colour| colour[0] <= 12), "{ten:?}");
        // Nothing asked for: one colour.
        assert_eq!(reduce_colors(&ramp, 0).0.len(), 1);
    }
}
