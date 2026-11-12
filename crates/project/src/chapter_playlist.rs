//! Chapter playlists (Encore's チャプター再生リスト): the chapters of one timeline, played in any order, as a title
//! of their own with an end action.
//!
//! On a DVD a chapter playlist is another PGC in the master timeline's title set that lists the same cells in the
//! chosen order (so it costs no disc space); on a Blu-ray it is another PlayList whose PlayItems are the chapter
//! spans of the master clip.

use crate::ids::TimelineId;
use crate::io::ProjectError;
use crate::link::Link;
use crate::model::Project;
use crate::user_ops::UserOp;
use serde::{Deserialize, Serialize};

/// The most entries in one chapter playlist (a DVD title has at most 99 programs).
pub const MAX_CHAPTER_PLAYLIST_ENTRIES: usize = 99;

/// How the entries play (Encore's 再生の種類).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackOrder {
    /// In the listed order.
    #[default]
    Sequential,
    /// A random entry each time, repeating until stopped (the DVD "random" PG playback mode).
    Random,
    /// Every entry once in a random order (the DVD "shuffle" PG playback mode).
    Shuffle,
}

impl PlaybackOrder {
    /// Every order, in the order of Encore's list.
    pub const ALL: [PlaybackOrder; 3] = [
        PlaybackOrder::Sequential,
        PlaybackOrder::Random,
        PlaybackOrder::Shuffle,
    ];

    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            PlaybackOrder::Sequential => "sequential",
            PlaybackOrder::Random => "random",
            PlaybackOrder::Shuffle => "shuffle",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|order| order.as_str() == text)
    }
}

fn is_sequential(order: &PlaybackOrder) -> bool {
    *order == PlaybackOrder::Sequential
}

/// One chapter playlist.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterPlaylist {
    /// Shared object id.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// The master timeline whose chapters play.
    pub timeline: TimelineId,
    /// Entries: zero-based chapter indexes of the master timeline, in play order. A chapter may appear more than
    /// once.
    #[serde(default)]
    pub chapters: Vec<u32>,
    /// Action at the end.
    #[serde(default)]
    pub end_action: Link,
    /// Optional override link (Encore's 上書き).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_link: Option<Link>,
    /// Free description (kept in the project only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Remote operations prohibited while it plays (DVD).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prohibited_ops: Vec<UserOp>,
    /// How the entries play.
    #[serde(default, skip_serializing_if = "is_sequential")]
    pub order: PlaybackOrder,
}

impl ChapterPlaylist {
    /// The entries as one-based program numbers of the master timeline, when every entry exists in `timeline`.
    pub fn program_numbers(&self, chapter_count: usize) -> Option<Vec<u16>> {
        self.chapters
            .iter()
            .map(|chapter| {
                let index = usize::try_from(*chapter).ok()?;
                (index < chapter_count).then(|| u16::try_from(index.checked_add(1)?).ok())?
            })
            .collect()
    }
}

impl Project {
    /// Adds a chapter playlist of `timeline`. Without a name it is called after the timeline, like Encore's
    /// "<timeline> Chapter Playlist".
    pub fn add_chapter_playlist(
        &mut self,
        timeline: TimelineId,
        name: Option<String>,
    ) -> Result<u64, ProjectError> {
        let master = self
            .timelines
            .get(&timeline)
            .ok_or_else(|| ProjectError::Invalid("timeline does not exist".to_string()))?;
        let name = name.unwrap_or_else(|| format!("{} Chapter Playlist", master.name));
        let id = self.alloc_id()?;
        self.chapter_playlists.insert(
            id,
            ChapterPlaylist {
                id,
                name,
                timeline,
                chapters: Vec::new(),
                end_action: Link::NotSet,
                override_link: None,
                description: String::new(),
                prohibited_ops: Vec::new(),
                order: PlaybackOrder::Sequential,
            },
        );
        Ok(id)
    }

    /// Borrows a chapter playlist.
    pub fn chapter_playlist(&self, id: u64) -> Option<&ChapterPlaylist> {
        self.chapter_playlists.get(&id)
    }

    /// Mutably borrows a chapter playlist.
    pub fn chapter_playlist_mut(&mut self, id: u64) -> Option<&mut ChapterPlaylist> {
        self.chapter_playlists.get_mut(&id)
    }

    /// Appends (or inserts at `at`) an entry for chapter `chapter` of the master timeline; returns its index.
    pub fn add_chapter_playlist_entry(
        &mut self,
        id: u64,
        chapter: u32,
        at: Option<usize>,
    ) -> Result<usize, ProjectError> {
        let chapter_count = self.chapter_playlist_master_chapters(id)?;
        let index = usize::try_from(chapter)
            .map_err(|_| ProjectError::Invalid("chapter index out of range".to_string()))?;
        if index >= chapter_count {
            return Err(ProjectError::Invalid(
                "chapter index out of range".to_string(),
            ));
        }
        let playlist = self.chapter_playlist_mut_or_err(id)?;
        if playlist.chapters.len() >= MAX_CHAPTER_PLAYLIST_ENTRIES {
            return Err(ProjectError::Invalid(
                "chapter playlist has too many entries".to_string(),
            ));
        }
        let at = at.unwrap_or(playlist.chapters.len());
        if at > playlist.chapters.len() {
            return Err(ProjectError::Invalid(
                "entry index out of range".to_string(),
            ));
        }
        playlist.chapters.insert(at, chapter);
        Ok(at)
    }

    /// Removes entry `entry`.
    pub fn remove_chapter_playlist_entry(
        &mut self,
        id: u64,
        entry: usize,
    ) -> Result<(), ProjectError> {
        let playlist = self.chapter_playlist_mut_or_err(id)?;
        if entry >= playlist.chapters.len() {
            return Err(ProjectError::Invalid(
                "entry index out of range".to_string(),
            ));
        }
        let _ = playlist.chapters.remove(entry);
        Ok(())
    }

    /// Moves entry `from` so that it sits at `to` (Encore's 再生の順番 arrows and drag).
    pub fn move_chapter_playlist_entry(
        &mut self,
        id: u64,
        from: usize,
        to: usize,
    ) -> Result<(), ProjectError> {
        let playlist = self.chapter_playlist_mut_or_err(id)?;
        if from >= playlist.chapters.len() || to >= playlist.chapters.len() {
            return Err(ProjectError::Invalid(
                "entry index out of range".to_string(),
            ));
        }
        let chapter = playlist.chapters.remove(from);
        playlist.chapters.insert(to, chapter);
        Ok(())
    }

    /// Sets how the entries play.
    pub fn set_chapter_playlist_order(
        &mut self,
        id: u64,
        order: PlaybackOrder,
    ) -> Result<(), ProjectError> {
        self.chapter_playlist_mut_or_err(id)?.order = order;
        Ok(())
    }

    /// The chapter playlists of `timeline`, in id order.
    pub fn chapter_playlists_of(&self, timeline: TimelineId) -> Vec<u64> {
        self.chapter_playlists
            .values()
            .filter(|playlist| playlist.timeline == timeline)
            .map(|playlist| playlist.id)
            .collect()
    }

    /// Keeps chapter playlists in step with a removed chapter of `timeline`: entries of the chapter go, later
    /// entries move down one.
    pub(crate) fn drop_chapter_from_playlists(&mut self, timeline: TimelineId, index: usize) {
        let Ok(removed) = u32::try_from(index) else {
            return;
        };
        for playlist in self.chapter_playlists.values_mut() {
            if playlist.timeline != timeline {
                continue;
            }
            playlist.chapters.retain(|chapter| *chapter != removed);
            for chapter in &mut playlist.chapters {
                if *chapter > removed {
                    *chapter = chapter.saturating_sub(1);
                }
            }
        }
    }

    fn chapter_playlist_master_chapters(&self, id: u64) -> Result<usize, ProjectError> {
        let playlist = self
            .chapter_playlists
            .get(&id)
            .ok_or_else(|| ProjectError::Invalid("chapter playlist does not exist".to_string()))?;
        self.timelines
            .get(&playlist.timeline)
            .map(|timeline| timeline.chapters.len())
            .ok_or_else(|| ProjectError::Invalid("master timeline does not exist".to_string()))
    }

    fn chapter_playlist_mut_or_err(
        &mut self,
        id: u64,
    ) -> Result<&mut ChapterPlaylist, ProjectError> {
        self.chapter_playlists
            .get_mut(&id)
            .ok_or_else(|| ProjectError::Invalid("chapter playlist does not exist".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_CHAPTER_PLAYLIST_ENTRIES, PlaybackOrder};
    use crate::model::Project;
    use filmcraft_time::Tick;
    use std::error::Error;
    use std::path::Path;

    fn project() -> Result<(Project, crate::ids::TimelineId), Box<dyn Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(Path::new("a.m2v"))?;
        let timeline = project.add_timeline(asset, Some("Feature".to_owned()))?;
        let _ = project.add_chapter(timeline, Tick(27_000_000 * 10 * 9408), None)?;
        let _ = project.add_chapter(timeline, Tick(27_000_000 * 20 * 9408), None)?;
        Ok((project, timeline))
    }

    #[test]
    fn entries_are_added_moved_and_removed() -> Result<(), Box<dyn Error>> {
        let (mut project, timeline) = project()?;
        let id = project.add_chapter_playlist(timeline, None)?;
        assert_eq!(
            project.chapter_playlist(id).map(|list| list.name.as_str()),
            Some("Feature Chapter Playlist")
        );

        assert_eq!(project.add_chapter_playlist_entry(id, 2, None)?, 0);
        assert_eq!(project.add_chapter_playlist_entry(id, 0, None)?, 1);
        assert_eq!(project.add_chapter_playlist_entry(id, 2, Some(0))?, 0);
        assert!(project.add_chapter_playlist_entry(id, 3, None).is_err());
        assert!(project.add_chapter_playlist_entry(id, 0, Some(9)).is_err());

        project.move_chapter_playlist_entry(id, 2, 0)?;
        let chapters = project
            .chapter_playlist(id)
            .map(|list| list.chapters.clone());
        assert_eq!(chapters, Some(vec![0, 2, 2]));
        assert_eq!(
            project
                .chapter_playlist(id)
                .and_then(|list| list.program_numbers(3)),
            Some(vec![1, 3, 3])
        );

        project.remove_chapter_playlist_entry(id, 1)?;
        assert!(project.remove_chapter_playlist_entry(id, 5).is_err());
        let chapters = project
            .chapter_playlist(id)
            .map(|list| list.chapters.clone());
        assert_eq!(chapters, Some(vec![0, 2]));

        project.set_chapter_playlist_order(id, PlaybackOrder::Shuffle)?;
        assert_eq!(
            project.chapter_playlist(id).map(|list| list.order),
            Some(PlaybackOrder::Shuffle)
        );
        Ok(())
    }

    #[test]
    fn the_entry_count_is_capped() -> Result<(), Box<dyn Error>> {
        let (mut project, timeline) = project()?;
        let id = project.add_chapter_playlist(timeline, None)?;
        for _ in 0..MAX_CHAPTER_PLAYLIST_ENTRIES {
            let _ = project.add_chapter_playlist_entry(id, 1, None)?;
        }
        assert!(project.add_chapter_playlist_entry(id, 1, None).is_err());
        Ok(())
    }

    #[test]
    fn removing_a_chapter_renumbers_entries() -> Result<(), Box<dyn Error>> {
        let (mut project, timeline) = project()?;
        let id = project.add_chapter_playlist(timeline, None)?;
        for chapter in [2, 1, 0, 2] {
            let _ = project.add_chapter_playlist_entry(id, chapter, None)?;
        }
        project.remove_chapter(timeline, 1)?;
        let chapters = project
            .chapter_playlist(id)
            .map(|list| list.chapters.clone());
        assert_eq!(chapters, Some(vec![1, 0, 1]));
        assert_eq!(
            project
                .chapter_playlist(id)
                .and_then(|list| list.program_numbers(2)),
            Some(vec![2, 1, 2])
        );
        Ok(())
    }

    #[test]
    fn playback_order_round_trips_its_names() {
        for order in PlaybackOrder::ALL {
            assert_eq!(PlaybackOrder::parse(order.as_str()), Some(order));
        }
        assert_eq!(PlaybackOrder::parse("loop"), None);
    }
}
