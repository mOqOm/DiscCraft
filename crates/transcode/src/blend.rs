//! Encore's "Use Frame Blending" (M6.327): where an output picture falls between two of the source's frames (the
//! source at another frame rate), it is the two mixed by how near it is to each, instead of the one before it.

use std::sync::Arc;

use filmcraft_frame::VideoFrame;
use filmcraft_time::Tick;

use crate::TranscodeError;
use crate::source::Source;

/// The picture at `time` of `source` as `render` makes it (RGB24): with `blend`, mixed with the next frame's
/// where `time` falls between the two ([`Source::between`]).
pub(crate) fn picture_at(
    source: &Source,
    time: Tick,
    blend: bool,
    render: &dyn Fn(&VideoFrame) -> Result<Vec<u8>, TranscodeError>,
) -> Result<Vec<u8>, TranscodeError> {
    let frame = source.video_frame(time)?;
    let picture = render(&frame)?;
    let Some((after, weight)) = blend.then(|| source.between(time)).flatten() else {
        return Ok(picture);
    };
    // Past what can be decoded: the frame alone.
    let Ok(next) = source.video_frame(after) else {
        return Ok(picture);
    };
    if Arc::ptr_eq(&frame, &next) {
        return Ok(picture);
    }
    let following = render(&next)?;
    Ok(mix(&picture, &following, weight))
}

/// `first` and `second` mixed, `weight` 256ths of `second` (as long as the shorter).
pub(crate) fn mix(first: &[u8], second: &[u8], weight: u32) -> Vec<u8> {
    let weight = weight.min(256);
    first
        .iter()
        .zip(second)
        .map(|(a, b)| {
            let value = (u32::from(*a) * (256 - weight) + u32::from(*b) * weight + 128) >> 8;
            u8::try_from(value).unwrap_or(u8::MAX)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::mix;

    #[test]
    fn pictures_mix_by_weight() {
        assert_eq!(mix(&[0, 255, 100], &[255, 0, 100], 0), [0, 255, 100]);
        assert_eq!(mix(&[0, 255, 100], &[255, 0, 100], 256), [255, 0, 100]);
        assert_eq!(mix(&[0, 255], &[255, 0], 128), [128, 128]);
        assert_eq!(mix(&[0, 200], &[255, 0], 64), [64, 150]);
    }
}
