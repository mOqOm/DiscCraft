//! The layer break of a dual-layer DVD image built from the project (M6.515): read back from the written image by the
//! build, and kept in the project (`DiscSettings::image_layer_break`) so that it is saved with it. The Build panel
//! shows it after the project is opened again, and writing that image to a dual-layer disc uses it. The build still
//! writes it beside the image (`<name>.layerbreak.txt`) as well, for the command line and other programs.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Where a dual-layer DVD image goes from layer 0 to layer 1, read back from the written image.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerBreak {
    /// The first sector of layer 1 in the image: the size of layer 0.
    pub sector: u32,
    /// The image's size in sectors.
    pub total_sectors: u32,
    /// Whether the project set it (手動) rather than the build choosing it (自動).
    pub manual: bool,
    /// The title and cell the image's video starts at the break, read back from the image; `None` when the sector
    /// is no title cell's start (a build never chooses such a sector).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<LayerBreakPlace>,
    /// For a manual break: the time asked for, at the frame nearest to it (90 kHz from the title's start).
    #[serde(
        default,
        rename = "requested90k",
        skip_serializing_if = "Option::is_none"
    )]
    pub requested_90k: Option<u64>,
}

/// The title and cell at a layer break.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerBreakPlace {
    /// The project's id of the title's timeline (a slideshow or chapter playlist id for those titles).
    pub timeline: u64,
    /// The title number (1-based).
    pub title: u8,
    /// The title set (1-based).
    pub vts: u8,
    /// The cell of the title's program chain (1-based) that starts at the break.
    pub cell: u8,
    /// The time from the title's start of the video at the break (90 kHz), from the navigation packs in the image.
    #[serde(rename = "time90k")]
    pub time_90k: u64,
}

impl LayerBreak {
    /// The size of layer 1 in sectors.
    #[must_use]
    pub fn layer1_sectors(&self) -> u32 {
        self.total_sectors.saturating_sub(self.sector)
    }

    /// For a manual break: whether the image breaks at the frame asked for.
    #[must_use]
    pub fn as_requested(&self) -> Option<bool> {
        let requested = self.requested_90k?;
        Some(
            self.place
                .as_ref()
                .is_some_and(|place| place.time_90k == requested),
        )
    }
}

/// The dual-layer DVD image last built from the project, and its layer break.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageLayerBreak {
    /// The image (its path; saved relative to the project file too, as `relativeImage`).
    pub image: String,
    /// The image's size in bytes: another file put at that path later is not taken for it.
    pub bytes: u64,
    /// Its layer break.
    #[serde(flatten)]
    pub found: LayerBreak,
}

/// The longest image path kept.
const MAX_IMAGE_PATH: usize = 4096;

/// `path` made absolute (against the current folder), as text.
fn absolute_text(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Whether two paths name the same file as far as their text tells (letters' case aside on Windows).
fn same_path(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}

impl crate::Project {
    /// Notes the image just built at `image` (`bytes` long) and its layer break `found` (M6.515): a dual-layer image
    /// is kept (in place of the one before); a single-layer image built where the kept one was replaces it, so it
    /// is forgotten. Returns whether the project changed.
    pub fn note_built_image(
        &mut self,
        image: &Path,
        bytes: u64,
        found: Option<LayerBreak>,
    ) -> bool {
        let text = absolute_text(image);
        let kept = &mut self.settings.image_layer_break;
        let next = match found {
            Some(found) if text.len() <= MAX_IMAGE_PATH => Some(ImageLayerBreak {
                image: text,
                bytes,
                found,
            }),
            Some(_) => None,
            None => match kept {
                Some(old) if same_path(&old.image, &text) => None,
                _ => return false,
            },
        };
        if *kept == next {
            return false;
        }
        *kept = next;
        true
    }

    /// The layer break the project keeps for the image at `image` (M6.515): when it is the image it built there,
    /// still that size; `None` otherwise.
    #[must_use]
    pub fn built_image_layer_break(&self, image: &Path) -> Option<&LayerBreak> {
        let kept = self.settings.image_layer_break.as_ref()?;
        if !same_path(&kept.image, &absolute_text(image)) {
            return None;
        }
        let bytes = std::fs::metadata(image).ok()?.len();
        (bytes == kept.bytes).then_some(&kept.found)
    }
}

#[cfg(test)]
mod tests {
    use super::{LayerBreak, LayerBreakPlace};
    use crate::Project;
    use std::error::Error;

    fn found(sector: u32) -> LayerBreak {
        LayerBreak {
            sector,
            total_sectors: 3_832_289,
            manual: true,
            place: Some(LayerBreakPlace {
                timeline: 7,
                title: 1,
                vts: 1,
                cell: 12,
                time_90k: 900_900,
            }),
            requested_90k: Some(900_900),
        }
    }

    /// M6.515: a dual-layer image is kept, found again only at its path and size, replaced by the next one, and
    /// forgotten when a single-layer image is built in its place; the record goes through the project file.
    #[test]
    fn the_built_image_and_its_layer_break_are_kept() -> Result<(), Box<dyn Error>> {
        let dir =
            std::env::temp_dir().join(format!("disccraft-built-image-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let image = dir.join("disc.iso");
        std::fs::write(&image, vec![0_u8; 4096])?;
        let mut project = Project::default();
        assert!(project.note_built_image(&image, 4096, Some(found(2_038_416))));
        assert!(!project.note_built_image(&image, 4096, Some(found(2_038_416))));
        assert_eq!(
            project
                .built_image_layer_break(&image)
                .map(|found| found.sector),
            Some(2_038_416)
        );
        // Another file at that path (a different size), or another path: not this image.
        std::fs::write(&image, vec![0_u8; 2048])?;
        assert_eq!(project.built_image_layer_break(&image), None);
        std::fs::write(&image, vec![0_u8; 4096])?;
        assert_eq!(
            project.built_image_layer_break(&dir.join("other.iso")),
            None
        );

        // Saved and loaded with the project.
        let file = dir.join("disc.dcproj");
        crate::save(&project, &file)?;
        let text = std::fs::read_to_string(&file)?;
        assert!(
            text.contains("\"imageLayerBreak\"")
                && text.contains("\"relativeImage\": \"disc.iso\""),
            "{text}"
        );
        assert!(
            text.contains("\"time90k\": 900900") && text.contains("\"requested90k\": 900900"),
            "{text}"
        );
        let loaded = crate::load(&file)?;
        assert_eq!(
            loaded.settings.image_layer_break,
            project.settings.image_layer_break
        );

        // A single-layer image elsewhere leaves it; one at its place forgets it.
        assert!(!project.note_built_image(&dir.join("small.iso"), 2048, None));
        assert!(project.settings.image_layer_break.is_some());
        assert!(project.note_built_image(&image, 2048, None));
        assert_eq!(project.settings.image_layer_break, None);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
