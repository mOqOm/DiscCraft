//! M6.548: a Blu-ray's parental chapters (M13.17's parental block, on a DVD the title's versions as PGCs). On a
//! Blu-ray each version below the top one is a playlist of the title's chapters with the alternative videos in place
//! of the chapters it replaces; the alternative videos become clips as hidden titles, which a Blu-ray build adds to
//! the project as this module does (and the preview of a built disc counts the same way).

use crate::ids::{AssetId, TimelineId};
use crate::io::ProjectError;
use crate::model::{AssetKind, DiscKind, Project};

/// The hidden title a parental chapter's alternative video plays from on a Blu-ray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentalClip {
    /// The timeline whose chapter it replaces.
    pub timeline: TimelineId,
    /// The alternative video.
    pub asset: AssetId,
    /// The hidden timeline of that video.
    pub hidden: TimelineId,
}

impl Project {
    /// The timelines of a Blu-ray with parental chapters whose alternatives are videos, in id order.
    fn bd_parental_timelines(&self) -> Vec<TimelineId> {
        if self.settings.kind != DiscKind::BluRay {
            return Vec::new();
        }
        self.timelines
            .values()
            .filter(|timeline| timeline.parental_variants().len() > 1)
            .map(|timeline| timeline.id)
            .collect()
    }

    /// A copy of a Blu-ray project for building, with a hidden timeline for each video that plays in place of a
    /// parental chapter (one for each timeline and video), and which they are. Any other project as it is.
    ///
    /// # Errors
    ///
    /// When a timeline cannot be added (too many).
    pub fn with_parental_clips(&self) -> Result<(Project, Vec<ParentalClip>), ProjectError> {
        let mut out = self.clone();
        let mut clips: Vec<ParentalClip> = Vec::new();
        for id in self.bd_parental_timelines() {
            let Some(timeline) = self.timelines.get(&id) else {
                continue;
            };
            for chapter in &timeline.parental_chapters {
                let video = self
                    .assets
                    .get(&chapter.alternative)
                    .is_some_and(|asset| asset.kind == AssetKind::Video);
                if !video
                    || clips
                        .iter()
                        .any(|clip| clip.timeline == id && clip.asset == chapter.alternative)
                {
                    continue;
                }
                let hidden = out.add_timeline(
                    chapter.alternative,
                    Some(format!("Parental: {}", timeline.name)),
                )?;
                clips.push(ParentalClip {
                    timeline: id,
                    asset: chapter.alternative,
                    hidden,
                });
            }
        }
        Ok((out, clips))
    }

    /// The playlists a Blu-ray build writes for the parental versions below the top one of its timelines (after the
    /// chapter playlists, before the menus), in their order: each one's timeline and version
    /// ([`crate::Timeline::parental_variants`], from 1). Only timelines with a video in place of a chapter have
    /// them, as the build writes them (M6.552: the preview of a built disc tells them apart by this).
    #[must_use]
    pub fn bd_parental_versions(&self) -> Vec<(TimelineId, usize)> {
        self.bd_parental_timelines()
            .into_iter()
            .filter_map(|id| self.timelines.get(&id))
            .filter(|timeline| {
                timeline.parental_chapters.iter().any(|chapter| {
                    self.assets
                        .get(&chapter.alternative)
                        .is_some_and(|asset| asset.kind == AssetKind::Video)
                })
            })
            .flat_map(|timeline| {
                (1..timeline.parental_variants().len()).map(move |variant| (timeline.id, variant))
            })
            .collect()
    }

    /// How many playlists a Blu-ray build writes for the parental versions ([`Self::bd_parental_versions`]).
    #[must_use]
    pub fn bd_parental_version_playlists(&self) -> usize {
        self.bd_parental_versions().len()
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{DiscKind, ParentalChapter, Project};
    use crate::{Tick, TimelineId};
    use std::error::Error;
    use std::path::Path;

    /// A Blu-ray's timeline with two parental chapters of one video gets one hidden timeline of it; the versions
    /// below the top one are counted; a DVD's is left as it is.
    #[test]
    fn parental_alternatives_become_hidden_timelines_on_a_blu_ray() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.settings.kind = DiscKind::BluRay;
        let movie = project.add_asset(Path::new("movie.m2v"))?;
        let other = project.add_asset(Path::new("other.m2v"))?;
        let id = project.add_timeline(movie, Some("Main".to_owned()))?;
        project.add_chapter(id, Tick(254_016_000_000), None)?;
        project.add_chapter(id, Tick(2 * 254_016_000_000), None)?;
        let timeline = project.timelines.get_mut(&id).ok_or("timeline")?;
        timeline.parental_chapters = vec![
            ParentalChapter {
                chapter: 1,
                level: 6,
                alternative: other,
            },
            ParentalChapter {
                chapter: 2,
                level: 4,
                alternative: other,
            },
        ];
        // Versions for 6-8, 4-5 and 1-3: two below the top.
        assert_eq!(project.bd_parental_version_playlists(), 2);
        assert_eq!(project.bd_parental_versions(), [(id, 1), (id, 2)]);
        let (expanded, clips) = project.with_parental_clips()?;
        assert_eq!(clips.len(), 1);
        assert_eq!((clips[0].timeline, clips[0].asset), (id, other));
        let hidden = expanded.timelines.get(&clips[0].hidden).ok_or("hidden")?;
        assert_eq!(hidden.video, other);
        assert_eq!(expanded.timelines.len(), 2);
        assert!(clips[0].hidden > TimelineId(id.0));
        // M6.552: with the alternative not a video, the build writes no versions, and none are counted.
        let image = project.add_asset(Path::new("still.png"))?;
        for chapter in &mut project
            .timelines
            .get_mut(&id)
            .ok_or("timeline")?
            .parental_chapters
        {
            chapter.alternative = image;
        }
        assert!(project.bd_parental_versions().is_empty());
        project.settings.kind = DiscKind::Dvd;
        assert_eq!(project.bd_parental_version_playlists(), 0);
        assert!(project.with_parental_clips()?.1.is_empty());
        Ok(())
    }
}
