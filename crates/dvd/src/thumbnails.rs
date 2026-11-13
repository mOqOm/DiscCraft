//! The pictures of a menu's video thumbnails (Encore's `(%)` layers) on a disc: each thumbnail's still picture
//! (its chapter's poster frame), read from its video.

use std::path::Path;

use disccraft_menu::ThumbnailPicture;
use disccraft_project::{Menu, Project};

/// One thumbnail's decoded picture (RGB24).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThumbnailFrame {
    /// The button's index.
    pub button: usize,
    /// The pixels.
    pub rgb: Vec<u8>,
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
}

impl ThumbnailFrame {
    /// The picture for the menu renderer.
    pub fn picture(&self) -> ThumbnailPicture<'_> {
        ThumbnailPicture {
            button: self.button,
            rgb: &self.rgb,
            width: self.width,
            height: self.height,
        }
    }
}

/// The still pictures of `menu`'s video thumbnails: the poster frame, or without one the chapter's first bright
/// (non-black) frame (Encore). A video that cannot be read leaves its thumbnail without a picture (the menu's own
/// pixels show there), as a missing picture should not stop a build.
pub fn still_thumbnails(project: &Project, menu: &Menu) -> Vec<ThumbnailFrame> {
    project
        .menu_thumbnails(menu)
        .into_iter()
        .filter_map(|thumbnail| {
            if thumbnail.source.still {
                return still_frame(project, thumbnail.button, thumbnail.source.asset);
            }
            let asset = project.assets.get(&thumbnail.source.asset)?;
            let reader = disccraft_transcode::FrameReader::open(Path::new(&asset.path)).ok()?;
            let start = thumbnail.source.start.seconds();
            let time = if thumbnail.source.poster {
                start
            } else {
                reader.first_bright_time(start).ok()?
            };
            // The picture without letterbox bars, stretched over the area (Encore).
            let (width, height, rgb) = reader.picture(time).ok()?;
            Some(ThumbnailFrame {
                button: thumbnail.button,
                rgb,
                width: usize::try_from(width).ok()?,
                height: usize::try_from(height).ok()?,
            })
        })
        .collect()
}

/// A still picture (a slideshow's first slide) for `button`'s thumbnail.
fn still_frame(
    project: &Project,
    button: usize,
    asset: disccraft_project::AssetId,
) -> Option<ThumbnailFrame> {
    let (rgb, width, height) = disccraft_menu::image_rgb(project, asset)?;
    Some(ThumbnailFrame {
        button,
        rgb,
        width,
        height,
    })
}

/// The renderer's view of `frames`.
pub(crate) fn pictures(frames: &[ThumbnailFrame]) -> Vec<ThumbnailPicture<'_>> {
    frames.iter().map(ThumbnailFrame::picture).collect()
}

/// The video thumbnails of a menu whose motion buttons play (Encore's モーションボタン): each one plays its video from
/// its still picture (the poster frame) to the video's end, then starts again from there. A still picture (a
/// slideshow's slide) stays.
pub(crate) struct MovingThumbnails {
    thumbnails: Vec<MovingThumbnail>,
    stills: Vec<ThumbnailFrame>,
}

struct MovingThumbnail {
    button: usize,
    reader: disccraft_transcode::FrameReader,
    /// Where it starts and loops back to, in seconds of its video.
    start: f64,
    /// How long one play is.
    span: f64,
}

impl MovingThumbnails {
    /// Opens the videos of `menu`'s thumbnails (a video that cannot be read leaves its thumbnail still and empty).
    pub(crate) fn open(project: &Project, menu: &Menu) -> Self {
        let all = project.menu_thumbnails(menu);
        let stills = all
            .iter()
            .filter(|thumbnail| thumbnail.source.still)
            .filter_map(|thumbnail| still_frame(project, thumbnail.button, thumbnail.source.asset))
            .collect();
        let thumbnails = all
            .into_iter()
            .filter(|thumbnail| !thumbnail.source.still)
            .filter_map(|thumbnail| {
                let asset = project.assets.get(&thumbnail.source.asset)?;
                let reader = disccraft_transcode::FrameReader::open(Path::new(&asset.path)).ok()?;
                let start = thumbnail.source.start.seconds().max(0.0);
                let span = reader.duration_seconds() - start;
                Some(MovingThumbnail {
                    button: thumbnail.button,
                    reader,
                    start,
                    span,
                })
            })
            .collect();
        Self { thumbnails, stills }
    }

    /// The thumbnails' pictures `elapsed` seconds into the menu.
    pub(crate) fn frames_at(&self, elapsed: f64) -> Vec<ThumbnailFrame> {
        self.thumbnails
            .iter()
            .filter_map(|thumbnail| {
                let time = loop_time(thumbnail.start, thumbnail.span, elapsed);
                let (width, height, rgb) = thumbnail.reader.picture(time).ok()?;
                Some(ThumbnailFrame {
                    button: thumbnail.button,
                    rgb,
                    width: usize::try_from(width).ok()?,
                    height: usize::try_from(height).ok()?,
                })
            })
            .chain(self.stills.iter().cloned())
            .collect()
    }
}

/// The time in a thumbnail's video `elapsed` seconds into the menu: from `start`, looping back to it every `span`
/// seconds (a span too short to play stays on the start).
pub(crate) fn loop_time(start: f64, span: f64, elapsed: f64) -> f64 {
    // Shorter than about a frame: nothing to play.
    const MIN_SPAN: f64 = 0.04;
    if !span.is_finite() || span < MIN_SPAN || !elapsed.is_finite() || elapsed <= 0.0 {
        return start;
    }
    start + elapsed.rem_euclid(span)
}

#[cfg(test)]
mod tests {
    use super::loop_time;

    #[test]
    fn a_thumbnail_loops_back_to_its_poster_frame() {
        // From 7 s in a 20 s video: 13 s to play, then 7 s again.
        assert!((loop_time(7.0, 13.0, 0.0) - 7.0).abs() < 1e-9);
        assert!((loop_time(7.0, 13.0, 5.0) - 12.0).abs() < 1e-9);
        assert!((loop_time(7.0, 13.0, 12.9) - 19.9).abs() < 1e-9);
        assert!((loop_time(7.0, 13.0, 13.1) - 7.1).abs() < 1e-9);
        // A poster frame at the very end, or a broken length, stays still.
        assert!((loop_time(20.0, 0.0, 5.0) - 20.0).abs() < 1e-9);
        assert!((loop_time(3.0, f64::NAN, 5.0) - 3.0).abs() < 1e-9);
    }
}
