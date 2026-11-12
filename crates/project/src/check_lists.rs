//! Slideshow, playlist and chapter playlist validation.

use crate::chapter_playlist::{ChapterPlaylist, PlaybackOrder};
use crate::check::{Problem, Severity};
use crate::ids::{AssetId, MenuId, TimelineId};
use crate::link::Link;
use crate::lists::{MAX_SLIDE_DURATION, MIN_SLIDE_DURATION, Playlist, Slideshow};
use crate::model::{AssetKind, DiscKind, Project};
use filmcraft_time::Tick;

/// Validates slideshows and playlists.
pub(crate) fn check_lists(project: &Project, problems: &mut Vec<Problem>) {
    for (id, slideshow) in &project.slideshows {
        check_slideshow(project, *id, slideshow, problems);
    }

    for (id, playlist) in &project.playlists {
        check_playlist(project, *id, playlist, problems);
    }

    for (id, playlist) in &project.chapter_playlists {
        check_chapter_playlist(project, *id, playlist, problems);
    }

    check_dvd_title_count(project, problems);
}

fn check_chapter_playlist(
    project: &Project,
    id: u64,
    playlist: &ChapterPlaylist,
    problems: &mut Vec<Problem>,
) {
    if id != playlist.id {
        problems.push(error(
            "idMismatch",
            format!(
                "chapter playlist key {id} does not match id {}",
                playlist.id
            ),
        ));
    }
    if id >= project.next_id {
        problems.push(error(
            "nextId",
            format!(
                "chapter playlist id {id} is not below next_id {}",
                project.next_id
            ),
        ));
    }

    match project.timelines.get(&playlist.timeline) {
        Some(timeline) => {
            for (entry, chapter) in playlist.chapters.iter().enumerate() {
                let index = usize::try_from(*chapter).unwrap_or(usize::MAX);
                if index >= timeline.chapters.len() {
                    problems.push(error(
                        "missingTarget",
                        format!(
                            "chapter playlist {id} entry {entry} targets missing chapter {chapter} of timeline {}",
                            playlist.timeline.0
                        ),
                    ));
                }
            }
        }
        None => problems.push(error(
            "missingTarget",
            format!(
                "chapter playlist {id} targets missing timeline {}",
                playlist.timeline.0
            ),
        )),
    }
    if playlist.chapters.len() > crate::chapter_playlist::MAX_CHAPTER_PLAYLIST_ENTRIES {
        problems.push(error(
            "tooManyEntries",
            format!("chapter playlist {id} has more than 99 entries"),
        ));
    }
    check_end_action(
        project,
        &playlist.end_action,
        "chapter playlist",
        id,
        problems,
    );

    if playlist.chapters.is_empty() {
        problems.push(warning(
            "emptyChapterPlaylist",
            format!("chapter playlist {id} has no entries"),
        ));
    }
    if playlist.order != PlaybackOrder::Sequential
        && project.settings.kind == crate::model::DiscKind::BluRay
    {
        problems.push(warning(
            "bluRayPlaybackOrder",
            format!(
                "chapter playlist {id}: Blu-ray discs play the entries in order (random and shuffle are DVD only)"
            ),
        ));
    }
}

fn check_slideshow(project: &Project, id: u64, slideshow: &Slideshow, problems: &mut Vec<Problem>) {
    check_slideshow_id(id, slideshow, project.next_id, problems);
    check_slides(project, id, slideshow, problems);
    check_slideshow_audio(project, id, slideshow, problems);
    check_end_action(project, &slideshow.end_action, "slideshow", id, problems);

    if slideshow.slides.is_empty() {
        problems.push(warning(
            "emptySlideshow",
            format!("slideshow {id} has no slides"),
        ));
    }
    if slideshow.manual_advance && project.settings.kind == crate::model::DiscKind::BluRay {
        problems.push(warning(
            "bluRayManualAdvance",
            format!(
                "slideshow {id}: Blu-ray discs do not apply manual advance yet (it is DVD only)"
            ),
        ));
    }
}

fn check_slideshow_id(id: u64, slideshow: &Slideshow, next_id: u64, problems: &mut Vec<Problem>) {
    if id != slideshow.id {
        problems.push(error(
            "idMismatch",
            format!("slideshow key {id} does not match id {}", slideshow.id),
        ));
    }

    if id >= next_id {
        problems.push(error(
            "nextId",
            format!("slideshow id {id} is not below next_id {next_id}"),
        ));
    }
}

fn check_slides(project: &Project, id: u64, slideshow: &Slideshow, problems: &mut Vec<Problem>) {
    for (index, slide) in slideshow.slides.iter().enumerate() {
        check_slide_asset(project, id, index, &slide.asset, problems);
        check_slide_duration(id, index, slide.duration, problems);
    }
}

fn check_slide_asset(
    project: &Project,
    id: u64,
    index: usize,
    asset: &AssetId,
    problems: &mut Vec<Problem>,
) {
    let valid = project
        .assets
        .get(asset)
        .is_some_and(|asset| asset.kind == AssetKind::Image);

    if !valid {
        problems.push(error(
            "missingAsset",
            format!(
                "slideshow {id} slide {index} references missing or non-image asset {}",
                asset.0
            ),
        ));
    }
}

fn check_slide_duration(id: u64, index: usize, duration: Tick, problems: &mut Vec<Problem>) {
    if duration < MIN_SLIDE_DURATION || duration > MAX_SLIDE_DURATION {
        problems.push(error(
            "badSlideDuration",
            format!("slideshow {id} slide {index} duration is outside 0.5s..=1h"),
        ));
    }
}

fn check_slideshow_audio(
    project: &Project,
    id: u64,
    slideshow: &Slideshow,
    problems: &mut Vec<Problem>,
) {
    for asset in slideshow.audio_list() {
        let valid = project
            .assets
            .get(&asset)
            .is_some_and(|asset| asset.kind == AssetKind::Audio);

        if !valid {
            problems.push(error(
                "missingAsset",
                format!(
                    "slideshow {id} audio asset {} is missing or not audio",
                    asset.0
                ),
            ));
        }
    }
}

fn check_playlist(project: &Project, id: u64, playlist: &Playlist, problems: &mut Vec<Problem>) {
    check_playlist_id(id, playlist, project.next_id, problems);
    check_playlist_items(project, id, playlist, problems);
    check_end_action(project, &playlist.end_action, "playlist", id, problems);

    if playlist.items.is_empty() {
        problems.push(warning(
            "emptyPlaylist",
            format!("playlist {id} has no items"),
        ));
    }
}

fn check_playlist_id(id: u64, playlist: &Playlist, next_id: u64, problems: &mut Vec<Problem>) {
    if id != playlist.id {
        problems.push(error(
            "idMismatch",
            format!("playlist key {id} does not match id {}", playlist.id),
        ));
    }

    if id >= next_id {
        problems.push(error(
            "nextId",
            format!("playlist id {id} is not below next_id {next_id}"),
        ));
    }
}

fn check_playlist_items(
    project: &Project,
    id: u64,
    playlist: &Playlist,
    problems: &mut Vec<Problem>,
) {
    for (index, item) in playlist.items.iter().enumerate() {
        if !item.exists_in(project) {
            let kind = match item {
                crate::lists::PlaylistItem::Timeline(_) => "timeline",
                crate::lists::PlaylistItem::Slideshow(_) => "slideshow",
                crate::lists::PlaylistItem::ChapterPlaylist(_) => "chapter playlist",
            };
            problems.push(error(
                "missingTarget",
                format!(
                    "playlist {id} item {index} targets missing {kind} {}",
                    item.id()
                ),
            ));
        }
    }
}

fn check_end_action(
    project: &Project,
    link: &Link,
    context: &str,
    id: u64,
    problems: &mut Vec<Problem>,
) {
    match link {
        Link::NotSet => {
            problems.push(warning(
                "endActionNotSet",
                format!("{context} {id} end action is not set"),
            ));
        }
        Link::Timeline {
            id: target,
            chapter,
        } => {
            check_timeline_target(project, target, *chapter, context, id, problems);
        }
        Link::Menu { id: target, .. } => {
            check_menu_target(project, target, context, id, problems);
        }
        Link::Playlist { id: target } => {
            check_playlist_target(project, target, context, id, problems);
        }
        Link::Slideshow { id: target } => {
            check_slideshow_target(project, target, context, id, problems);
        }
        Link::ChapterPlaylist { id: target } => {
            if !project.chapter_playlists.contains_key(target) {
                push_missing_target(
                    context,
                    id,
                    format!("chapter playlist {target} does not exist"),
                    problems,
                );
            }
        }
        Link::Stop | Link::LastMenu => {}
    }
}

fn check_timeline_target(
    project: &Project,
    target: &TimelineId,
    chapter: u32,
    context: &str,
    id: u64,
    problems: &mut Vec<Problem>,
) {
    let Some(timeline) = project.timelines.get(target) else {
        push_missing_target(
            context,
            id,
            format!("timeline {} does not exist", target.0),
            problems,
        );
        return;
    };

    let chapter_index = usize::try_from(chapter).unwrap_or(usize::MAX);
    if chapter_index >= timeline.chapters.len() {
        push_missing_target(
            context,
            id,
            format!("timeline {} has no chapter {}", target.0, chapter),
            problems,
        );
    }
}

fn check_menu_target(
    project: &Project,
    target: &MenuId,
    context: &str,
    id: u64,
    problems: &mut Vec<Problem>,
) {
    if !project.menus.contains_key(target) {
        push_missing_target(
            context,
            id,
            format!("menu {} does not exist", target.0),
            problems,
        );
    }
}

fn check_playlist_target(
    project: &Project,
    target: &u64,
    context: &str,
    id: u64,
    problems: &mut Vec<Problem>,
) {
    if !project.playlists.contains_key(target) {
        push_missing_target(
            context,
            id,
            format!("playlist {target} does not exist"),
            problems,
        );
    }
}

fn check_slideshow_target(
    project: &Project,
    target: &u64,
    context: &str,
    id: u64,
    problems: &mut Vec<Problem>,
) {
    if !project.slideshows.contains_key(target) {
        push_missing_target(
            context,
            id,
            format!("slideshow {target} does not exist"),
            problems,
        );
    }
}

fn push_missing_target(context: &str, id: u64, detail: String, problems: &mut Vec<Problem>) {
    problems.push(error(
        "missingTarget",
        format!("{context} {id} end action targets {detail}"),
    ));
}

fn check_dvd_title_count(project: &Project, problems: &mut Vec<Problem>) {
    if project.settings.kind != DiscKind::Dvd {
        return;
    }

    let count = project
        .timelines
        .len()
        .saturating_add(project.slideshows.len())
        .saturating_add(project.chapter_playlists.len());

    if count > 99 {
        problems.push(error(
            "tooManyTitles",
            format!("DVD supports at most 99 titles, found {count}"),
        ));
    }
}

fn error(code: &str, message: String) -> Problem {
    Problem {
        severity: Severity::Error,
        code: code.to_string(),
        message,
    }
}

fn warning(code: &str, message: String) -> Problem {
    Problem {
        severity: Severity::Warning,
        code: code.to_string(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::check_lists;
    use crate::check::{Problem, Severity};
    use crate::ids::{AssetId, TimelineId};
    use crate::link::Link;
    use crate::lists::{Playlist, Slide, Slideshow};
    use crate::model::{DiscKind, Project};
    use filmcraft_time::Tick;
    use std::error::Error;
    use std::path::Path;

    fn problems_for(project: &Project) -> Vec<Problem> {
        let mut problems = Vec::new();
        check_lists(project, &mut problems);
        problems
    }

    fn has_error_code(problems: &[Problem], code: &str) -> bool {
        problems
            .iter()
            .any(|problem| problem.code == code && problem.severity == Severity::Error)
    }

    fn has_warning_code(problems: &[Problem], code: &str) -> bool {
        problems
            .iter()
            .any(|problem| problem.code == code && problem.severity == Severity::Warning)
    }

    #[test]
    fn valid_lists_have_no_errors() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let image = project.add_asset(Path::new("slide.png"))?;
        let video = project.add_asset(Path::new("movie.mp4"))?;
        let timeline = project.add_timeline(video, None)?;
        let slideshow = project.add_slideshow(None)?;
        let playlist = project.add_playlist(None)?;

        project.add_slide(slideshow, image, Tick(5000 * 254_016_000))?;
        project.add_playlist_item(playlist, timeline)?;

        let problems = problems_for(&project);
        assert!(
            !problems
                .iter()
                .any(|problem| problem.severity == Severity::Error)
        );

        Ok(())
    }

    #[test]
    fn slideshow_id_mismatch() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.slideshows.insert(
            1,
            Slideshow {
                id: 2,
                name: "S".to_string(),
                slides: Vec::new(),
                audio: None,
                end_action: Link::NotSet,
                description: String::new(),
                slide_duration: crate::lists::DEFAULT_SLIDE_DURATION,
                transition: crate::lists::TransitionKind::None,
                transition_duration: crate::lists::DEFAULT_TRANSITION_DURATION,
                manual_advance: false,
                audio_clips: Vec::new(),
                loop_audio: false,
                fit_to_audio: false,
            },
        );

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "idMismatch"));

        Ok(())
    }

    #[test]
    fn slideshow_next_id() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.slideshows.insert(
            1,
            Slideshow {
                id: 1,
                name: "S".to_string(),
                slides: Vec::new(),
                audio: None,
                end_action: Link::NotSet,
                description: String::new(),
                slide_duration: crate::lists::DEFAULT_SLIDE_DURATION,
                transition: crate::lists::TransitionKind::None,
                transition_duration: crate::lists::DEFAULT_TRANSITION_DURATION,
                manual_advance: false,
                audio_clips: Vec::new(),
                loop_audio: false,
                fit_to_audio: false,
            },
        );

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "nextId"));

        Ok(())
    }

    #[test]
    fn playlist_id_mismatch() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.playlists.insert(
            1,
            Playlist {
                id: 2,
                name: "P".to_string(),
                items: Vec::new(),
                end_action: Link::NotSet,
            },
        );

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "idMismatch"));

        Ok(())
    }

    #[test]
    fn playlist_next_id() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.playlists.insert(
            1,
            Playlist {
                id: 1,
                name: "P".to_string(),
                items: Vec::new(),
                end_action: Link::NotSet,
            },
        );

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "nextId"));

        Ok(())
    }

    #[test]
    fn slide_missing_asset() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let id = project.add_slideshow(None)?;
        project
            .slideshow_mut(id)
            .ok_or("missing slideshow")?
            .slides
            .push(Slide {
                asset: AssetId(99),
                duration: Tick(5000 * 254_016_000),
                scale: crate::lists::SlideScale::ScaleAndMatte,
                transition: None,
            });

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "missingAsset"));

        Ok(())
    }

    #[test]
    fn slide_not_image() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let video = project.add_asset(Path::new("movie.mp4"))?;
        let id = project.add_slideshow(None)?;
        project
            .slideshow_mut(id)
            .ok_or("missing slideshow")?
            .slides
            .push(Slide {
                asset: video,
                duration: Tick(5000 * 254_016_000),
                scale: crate::lists::SlideScale::ScaleAndMatte,
                transition: None,
            });

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "missingAsset"));

        Ok(())
    }

    #[test]
    fn bad_slide_duration() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let image = project.add_asset(Path::new("slide.png"))?;
        let id = project.add_slideshow(None)?;
        project
            .slideshow_mut(id)
            .ok_or("missing slideshow")?
            .slides
            .push(Slide {
                asset: image,
                duration: Tick(100 * 254_016_000),
                scale: crate::lists::SlideScale::ScaleAndMatte,
                transition: None,
            });

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "badSlideDuration"));

        Ok(())
    }

    #[test]
    fn slideshow_audio_missing() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let id = project.add_slideshow(None)?;
        project.slideshow_mut(id).ok_or("missing slideshow")?.audio = Some(AssetId(99));

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "missingAsset"));

        Ok(())
    }

    #[test]
    fn slideshow_audio_not_audio() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let video = project.add_asset(Path::new("movie.mp4"))?;
        let id = project.add_slideshow(None)?;
        project.slideshow_mut(id).ok_or("missing slideshow")?.audio = Some(video);

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "missingAsset"));

        Ok(())
    }

    #[test]
    fn playlist_item_missing() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let id = project.add_playlist(None)?;
        project
            .playlist_mut(id)
            .ok_or("missing playlist")?
            .items
            .push(crate::lists::PlaylistItem::Timeline(TimelineId(99)));

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "missingTarget"));

        Ok(())
    }

    #[test]
    fn end_action_missing_target() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let id = project.add_slideshow(None)?;
        project
            .slideshow_mut(id)
            .ok_or("missing slideshow")?
            .end_action = Link::Timeline {
            id: TimelineId(99),
            chapter: 0,
        };

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "missingTarget"));

        Ok(())
    }

    #[test]
    fn too_many_titles() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.settings.kind = DiscKind::Dvd;

        for _ in 0..100 {
            project.add_slideshow(None)?;
        }

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "tooManyTitles"));

        Ok(())
    }

    #[test]
    fn empty_slideshow_warning() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.add_slideshow(None)?;

        let problems = problems_for(&project);
        assert!(has_warning_code(&problems, "emptySlideshow"));

        Ok(())
    }

    #[test]
    fn empty_playlist_warning() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.add_playlist(None)?;

        let problems = problems_for(&project);
        assert!(has_warning_code(&problems, "emptyPlaylist"));

        Ok(())
    }

    #[test]
    fn end_action_not_set_warning() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.add_slideshow(None)?;

        let problems = problems_for(&project);
        assert!(has_warning_code(&problems, "endActionNotSet"));

        Ok(())
    }
}
