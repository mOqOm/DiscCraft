//! The pixel aspect ratio a file's pictures are taken to have when the project says so (Encore's Interpret
//! Footage: "Specify pixel aspect ratio"), instead of what the file says.
//!
//! As with the chosen compositions (`crate::session`), the engine and a build pass the project's choices here, so
//! that every use of a file (its pictures in the Monitor, whether it is 16:9, a transcode) sees them the same way.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::TranscodeError;

/// The files with a pixel aspect ratio of the project's choosing (by full path): width over height of a pixel.
static CHOSEN: Mutex<Vec<(PathBuf, (u32, u32))>> = Mutex::new(Vec::new());

/// Says which files' pictures have a pixel aspect ratio other than their own. The list replaces the one before.
pub fn set_pixel_aspects(chosen: Vec<(PathBuf, (u32, u32))>) {
    let chosen = chosen
        .into_iter()
        .filter(|(_, (width, height))| *width > 0 && *height > 0)
        .map(|(path, aspect)| (std::path::absolute(&path).unwrap_or(path), aspect))
        .collect();
    *CHOSEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = chosen;
}

/// The pixel aspect ratio chosen for the file at `path`, if one is.
#[must_use]
pub fn chosen_pixel_aspect(path: &Path) -> Option<(u32, u32)> {
    let chosen = CHOSEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if chosen.is_empty() {
        return None;
    }
    let absolute = std::path::absolute(path).ok();
    let path = absolute.as_deref().unwrap_or(path);
    chosen
        .iter()
        .find(|(file, _)| file == path)
        .map(|(_, aspect)| *aspect)
}

/// The pixel aspect ratio Encore takes a still whose file says none to have, by its size: a DVD frame's (720 x 480
/// or 486, 720 x 576) has SD pixels (Encore's Interpret Footage of a 720 x 480 PNG: "SD NTSC (0.9091)"), so it
/// lies on a 4:3 DVD frame pixel for pixel: the 4:3 frame's own pixel here (8:9 NTSC, 16:15 PAL; Adobe's 0.9091
/// counts 704 of the 720 pixels as the picture, which would leave the still a few lines short of the frame). Any
/// other size has square pixels.
#[must_use]
pub fn still_default_aspect(width: u32, height: u32) -> (u32, u32) {
    match (width, height) {
        (720, 480) => (8, 9),
        (720, 486) => (9, 10),
        (720, 576) => (16, 15),
        _ => (1, 1),
    }
}

/// The pixel aspect ratio a still of `width` x `height` whose file says `file` is taken to have when the project
/// chose none: what the file says, else Encore's default for its size.
#[must_use]
pub fn still_own_aspect((width, height): (u32, u32), file: (u32, u32)) -> (u32, u32) {
    if file.0 > 0 && file.1 > 0 && file != (1, 1) {
        file
    } else {
        still_default_aspect(width, height)
    }
}

/// The pixel aspect ratio the still `input` has of its own (what its file says, else Encore's default for its
/// size), whatever is chosen for it.
///
/// # Errors
///
/// When the file cannot be opened as a picture.
pub fn still_pixel_aspect(input: &Path) -> Result<(u32, u32), TranscodeError> {
    let source = crate::source::Source::open(input)?;
    let video = source
        .info()
        .video
        .as_ref()
        .ok_or_else(|| TranscodeError::Unsupported("not a picture".to_string()))?;
    Ok(still_own_aspect((video.width, video.height), video.par))
}

/// The pixel aspect ratio the video file `input` says its pictures have (width over height of a pixel; square
/// when it says none), whatever is chosen for it.
///
/// # Errors
///
/// When the file cannot be opened as video.
pub fn pixel_aspect(input: &Path) -> Result<(u32, u32), TranscodeError> {
    let source = crate::source::Source::open(input)?;
    let aspect = source
        .info()
        .video
        .as_ref()
        .map_or((1, 1), |video| video.par);
    Ok(if aspect.0 == 0 || aspect.1 == 0 {
        (1, 1)
    } else {
        aspect
    })
}

#[cfg(test)]
mod tests {
    use super::{chosen_pixel_aspect, set_pixel_aspects, still_default_aspect, still_own_aspect};
    use std::path::Path;

    /// M6.320: a still the size of a DVD frame has SD pixels (Encore's "SD NTSC (0.9091)" for 720 x 480); others
    /// square; a file that says its own shape keeps it.
    #[test]
    fn a_dvd_sized_still_has_sd_pixels() {
        // Each a 4:3 picture.
        for ((width, height), (num, den)) in [
            ((720, 480), (8, 9)),
            ((720, 486), (9, 10)),
            ((720, 576), (16, 15)),
        ] {
            assert_eq!(still_default_aspect(width, height), (num, den));
            assert_eq!(width * num * 3, height * den * 4);
        }
        assert_eq!(still_default_aspect(640, 480), (1, 1));
        assert_eq!(still_default_aspect(1920, 1080), (1, 1));
        assert_eq!(still_own_aspect((720, 480), (1, 1)), (8, 9));
        assert_eq!(still_own_aspect((720, 480), (40, 33)), (40, 33));
        assert_eq!(still_own_aspect((640, 480), (0, 1)), (1, 1));
    }

    #[test]
    fn a_chosen_aspect_is_found_by_the_files_full_path() {
        // (One test owns the list: it is the process's.)
        assert_eq!(chosen_pixel_aspect(Path::new("clip.mp4")), None);
        set_pixel_aspects(vec![
            ("clip.mp4".into(), (40, 33)),
            ("bad.mp4".into(), (0, 1)),
        ]);
        let full = std::path::absolute("clip.mp4").unwrap_or_default();
        assert_eq!(chosen_pixel_aspect(&full), Some((40, 33)));
        assert_eq!(chosen_pixel_aspect(Path::new("clip.mp4")), Some((40, 33)));
        assert_eq!(chosen_pixel_aspect(Path::new("other.mp4")), None);
        assert_eq!(chosen_pixel_aspect(Path::new("bad.mp4")), None);
        set_pixel_aspects(Vec::new());
        assert_eq!(chosen_pixel_aspect(&full), None);
    }
}
