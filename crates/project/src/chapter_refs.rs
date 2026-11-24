//! What names a chapter of a timeline by its index, kept on the same chapter when the timeline's chapters change
//! (M6.428).
//!
//! Links (`Link::Timeline { chapter }`: buttons, end actions, first play, overrides, remotes), chapter playlist
//! entries and parental chapters name a chapter by its index. Encore's point at the chapter marker itself: a
//! chapter added before one, or one moved past it, does not change what they play. Before M6.428 only removing a
//! chapter renumbered the chapter playlists; adding or moving one left every index where it was, so a button or a
//! chapter playlist entry silently played the chapter that now had its old number.

use crate::ids::TimelineId;
use crate::link::Link;
use crate::model::Project;

impl Project {
    /// Renumbers everything that names a chapter of `timeline`: `map` gives each old index its new one, `None`
    /// for a chapter that went. A link to a chapter that went is no longer set (as a link to a removed menu or
    /// timeline); chapter playlist entries and parental chapters of it go.
    pub(crate) fn remap_chapters(
        &mut self,
        timeline: TimelineId,
        map: &dyn Fn(usize) -> Option<usize>,
    ) {
        let index = |chapter: u32| -> Option<u32> {
            usize::try_from(chapter)
                .ok()
                .and_then(map)
                .and_then(|new| u32::try_from(new).ok())
        };
        self.visit_links(|link| {
            if let Link::Timeline { id, chapter } = link
                && *id == timeline
            {
                match index(*chapter) {
                    Some(new) => *chapter = new,
                    None => *link = Link::NotSet,
                }
            }
        });
        for playlist in self.chapter_playlists.values_mut() {
            if playlist.timeline == timeline {
                playlist.chapters = playlist
                    .chapters
                    .iter()
                    .filter_map(|chapter| index(*chapter))
                    .collect();
            }
        }
        if let Some(found) = self.timelines.get_mut(&timeline) {
            found
                .parental_chapters
                .retain_mut(|parental| match map(parental.chapter) {
                    Some(new) => {
                        parental.chapter = new;
                        true
                    }
                    None => false,
                });
        }
    }

    /// Puts the chapters of `timeline` back in time order after their times changed, one chapter for each time
    /// (the first of chapters at the same time stays and the others' references go to it), with the references
    /// renumbered.
    pub(crate) fn sort_chapters(&mut self, timeline: TimelineId) {
        let Some(found) = self.timelines.get_mut(&timeline) else {
            return;
        };
        let mut order: Vec<usize> = (0..found.chapters.len()).collect();
        // Stable: of chapters at the same time, the one first before stays first.
        order.sort_by_key(|at| found.chapters.get(*at).map(|chapter| chapter.time));
        let mut map = vec![0usize; found.chapters.len()];
        let mut kept = Vec::with_capacity(found.chapters.len());
        for at in order {
            let Some(chapter) = found.chapters.get(at) else {
                continue;
            };
            match kept.last() {
                Some(last) if *last == chapter.time => {}
                _ => kept.push(chapter.time),
            }
            if let Some(slot) = map.get_mut(at) {
                *slot = kept.len().saturating_sub(1);
            }
        }
        let mut chapters = std::mem::take(&mut found.chapters);
        chapters.sort_by_key(|chapter| chapter.time);
        chapters.dedup_by_key(|chapter| chapter.time);
        found.chapters = chapters;
        if map.iter().enumerate().any(|(old, new)| old != *new) {
            self.remap_chapters(timeline, &|old| map.get(old).copied());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ids::TimelineId;
    use crate::link::Link;
    use crate::model::{ParentalChapter, Project};
    use filmcraft_time::{TICKS_PER_SECOND, Tick};
    use std::error::Error;
    use std::path::Path;

    /// A timeline with chapters at 0, 10, 20 and 30 s ("A" to "D"), a menu with a button to each chapter, a
    /// chapter playlist of C and D, and a parental chapter on C.
    fn project() -> Result<(Project, TimelineId, crate::ids::MenuId, u64), Box<dyn Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(Path::new("a.m2v"))?;
        let timeline = project.add_timeline(asset, None)?;
        project.rename_chapter(timeline, 0, "A")?;
        for (seconds, name) in [(10, "B"), (20, "C"), (30, "D")] {
            let _ = project.add_chapter(
                timeline,
                Tick(seconds * TICKS_PER_SECOND),
                Some(name.to_owned()),
            )?;
        }
        let menu = project.add_menu(Some("chapters".to_owned()))?;
        for chapter in 0..4 {
            let _ = project.add_button(
                menu,
                format!("button {chapter}"),
                None,
                Link::Timeline {
                    id: timeline,
                    chapter,
                },
            )?;
        }
        let playlist = project.add_chapter_playlist(timeline, None)?;
        let _ = project.add_chapter_playlist_entry(playlist, 2, None)?;
        let _ = project.add_chapter_playlist_entry(playlist, 3, None)?;
        project.settings.first_play = Link::Timeline {
            id: timeline,
            chapter: 3,
        };
        let asset = project.add_asset(Path::new("other.m2v"))?;
        if let Some(found) = project.timelines.get_mut(&timeline) {
            found.parental_chapters = vec![ParentalChapter {
                chapter: 2,
                level: 4,
                alternative: asset,
            }];
        }
        Ok((project, timeline, menu, playlist))
    }

    /// The names of the chapters each reference plays: the buttons, the chapter playlist, first play, the
    /// parental chapter.
    fn named(
        project: &Project,
        timeline: TimelineId,
        menu: crate::ids::MenuId,
        playlist: u64,
    ) -> (Vec<String>, Vec<String>, String, Vec<String>) {
        let found = project.timeline(timeline).unwrap();
        let name = |chapter: usize| {
            found
                .chapters
                .get(chapter)
                .map_or_else(|| "?".to_owned(), |chapter| chapter.name.clone())
        };
        let link = |link: &Link| match link {
            Link::Timeline { chapter, .. } => name(*chapter as usize),
            Link::NotSet => "not set".to_owned(),
            other => format!("{other:?}"),
        };
        (
            project.menus[&menu]
                .buttons
                .iter()
                .map(|button| link(&button.link))
                .collect(),
            project.chapter_playlists[&playlist]
                .chapters
                .iter()
                .map(|chapter| name(*chapter as usize))
                .collect(),
            link(&project.settings.first_play),
            found
                .parental_chapters
                .iter()
                .map(|parental| name(parental.chapter))
                .collect(),
        )
    }

    #[test]
    fn references_stay_on_their_chapter_when_one_is_added_moved_or_removed()
    -> Result<(), Box<dyn Error>> {
        let (mut project, timeline, menu, playlist) = project()?;
        let before = named(&project, timeline, menu, playlist);
        assert_eq!(
            before,
            (
                vec!["A".into(), "B".into(), "C".into(), "D".into()],
                vec!["C".into(), "D".into()],
                "D".into(),
                vec!["C".into()]
            )
        );

        // A chapter added before C and D.
        let _ = project.add_chapter(timeline, Tick(5 * TICKS_PER_SECOND), Some("new".into()))?;
        assert_eq!(named(&project, timeline, menu, playlist), before);

        // B moved past D, and D before everything but A.
        let _ = project.move_chapter(timeline, 2, Tick(40 * TICKS_PER_SECOND))?;
        assert_eq!(named(&project, timeline, menu, playlist), before);
        let d = project
            .timeline(timeline)
            .unwrap()
            .chapters
            .iter()
            .position(|chapter| chapter.name == "D")
            .unwrap();
        let _ = project.move_chapter(timeline, d, Tick(TICKS_PER_SECOND))?;
        assert_eq!(named(&project, timeline, menu, playlist), before);

        // C removed: its button is not set any more, its entry and parental chapter go; D's stay on D.
        let c = project
            .timeline(timeline)
            .unwrap()
            .chapters
            .iter()
            .position(|chapter| chapter.name == "C")
            .unwrap();
        project.remove_chapter(timeline, c)?;
        assert_eq!(
            named(&project, timeline, menu, playlist),
            (
                vec!["A".into(), "B".into(), "not set".into(), "D".into()],
                vec!["D".into()],
                "D".into(),
                vec![]
            )
        );
        Ok(())
    }

    #[test]
    fn sorting_after_times_change_follows_the_chapters() -> Result<(), Box<dyn Error>> {
        let (mut project, timeline, menu, playlist) = project()?;
        let before = named(&project, timeline, menu, playlist);
        // C to 35 s (past D) and B on to D's time: B and D become one chapter, D (first in order) stays.
        if let Some(found) = project.timelines.get_mut(&timeline) {
            found.chapters[2].time = Tick(35 * TICKS_PER_SECOND);
            found.chapters[1].time = Tick(30 * TICKS_PER_SECOND);
        }
        project.sort_chapters(timeline);
        let names: Vec<String> = project
            .timeline(timeline)
            .unwrap()
            .chapters
            .iter()
            .map(|chapter| chapter.name.clone())
            .collect();
        assert_eq!(names, ["A", "B", "C"]);
        // B (at 30 s, before D in the old order) stayed; D's references went to it.
        let after = named(&project, timeline, menu, playlist);
        assert_eq!(after.0, ["A", "B", "C", "B"]);
        assert_eq!(after.1, ["C", "B"]);
        assert_eq!(after.2, "B");
        assert_eq!(after.3, before.3);
        Ok(())
    }
}
