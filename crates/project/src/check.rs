use crate::link::Link;
use crate::model::{DiscKind, Project};
use serde::{Deserialize, Serialize};

/// Problem severity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// Blocking problem.
    Error,
    /// Non-blocking problem.
    Warning,
}

/// A validation problem.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// Problem severity.
    pub severity: Severity,
    /// Stable machine-readable code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

/// Return true if any problem is an error.
pub fn has_errors(problems: &[Problem]) -> bool {
    problems.iter().any(|p| p.severity == Severity::Error)
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

impl Project {
    /// Validate the project and return all problems.
    pub fn check(&self) -> Vec<Problem> {
        let mut problems = Vec::new();
        self.check_ids(&mut problems);
        self.check_timelines(&mut problems);
        self.check_links(&mut problems);
        self.check_dvd_limits(&mut problems);
        self.check_warnings(&mut problems);
        crate::check_menu::check_menus(self, &mut problems);
        crate::check_lists::check_lists(self, &mut problems);
        problems
    }

    fn check_ids(&self, problems: &mut Vec<Problem>) {
        for (id, asset) in &self.assets {
            if *id != asset.id {
                problems.push(error(
                    "idMismatch",
                    format!("asset key {} does not match id {}", id.0, asset.id.0),
                ));
            }
            if id.0 >= self.next_id {
                problems.push(error(
                    "nextId",
                    format!("asset id {} is not below next_id {}", id.0, self.next_id),
                ));
            }
        }

        for (id, timeline) in &self.timelines {
            if *id != timeline.id {
                problems.push(error(
                    "idMismatch",
                    format!("timeline key {} does not match id {}", id.0, timeline.id.0),
                ));
            }
            if id.0 >= self.next_id {
                problems.push(error(
                    "nextId",
                    format!("timeline id {} is not below next_id {}", id.0, self.next_id),
                ));
            }
        }

        for (id, menu) in &self.menus {
            if *id != menu.id {
                problems.push(error(
                    "idMismatch",
                    format!("menu key {} does not match id {}", id.0, menu.id.0),
                ));
            }
            if id.0 >= self.next_id {
                problems.push(error(
                    "nextId",
                    format!("menu id {} is not below next_id {}", id.0, self.next_id),
                ));
            }
        }
    }

    fn check_timelines(&self, problems: &mut Vec<Problem>) {
        for timeline in self.timelines.values() {
            if !self.assets.contains_key(&timeline.video) {
                problems.push(error(
                    "missingAsset",
                    format!(
                        "timeline {} references missing video asset {}",
                        timeline.id.0, timeline.video.0
                    ),
                ));
            }

            for track in &timeline.audio {
                if !self.assets.contains_key(&track.asset) {
                    problems.push(error(
                        "missingAsset",
                        format!(
                            "timeline {} references missing audio asset {}",
                            timeline.id.0, track.asset.0
                        ),
                    ));
                }
            }

            for track in &timeline.subtitles {
                if !self.assets.contains_key(&track.asset) {
                    problems.push(error(
                        "missingAsset",
                        format!(
                            "timeline {} references missing subtitle asset {}",
                            timeline.id.0, track.asset.0
                        ),
                    ));
                }
            }

            match timeline.chapters.first() {
                Some(first) if first.time == crate::Tick::ZERO => {}
                _ => problems.push(error(
                    "firstChapter",
                    format!(
                        "timeline {} must start with a chapter at time zero",
                        timeline.id.0
                    ),
                )),
            }

            let mut previous: Option<crate::Tick> = None;
            for chapter in &timeline.chapters {
                if let Some(prev) = previous
                    && chapter.time <= prev
                {
                    problems.push(error(
                        "chapterOrder",
                        format!("timeline {} has non-ascending chapter times", timeline.id.0),
                    ));
                    break;
                }
                previous = Some(chapter.time);
            }
        }
    }

    fn check_links(&self, problems: &mut Vec<Problem>) {
        self.check_link(&self.settings.first_play, problems);
        self.check_link(&self.settings.title_remote, problems);
        if let Some(link) = &self.settings.first_play_override {
            self.check_link(link, problems);
        }

        for menu in self.menus.values() {
            self.check_link(&menu.end_action, problems);
            if let Some(link) = &menu.override_link {
                self.check_link(link, problems);
            }
            for button in &menu.buttons {
                if let Some(link) = &button.override_link {
                    self.check_link(link, problems);
                }
            }
        }
        for timeline in self.timelines.values() {
            self.check_link(&timeline.end_action, problems);
            self.check_link(&timeline.menu_remote, problems);
            if let Some(link) = &timeline.override_link {
                self.check_link(link, problems);
            }
        }
        for playlist in self.chapter_playlists.values() {
            if let Some(link) = &playlist.override_link {
                self.check_link(link, problems);
            }
        }
    }

    /// Whether `link` points at something that exists in this project (a pasted button from another project may
    /// not).
    pub fn link_resolves(&self, link: &Link) -> bool {
        let mut problems = Vec::new();
        self.check_link(link, &mut problems);
        problems.is_empty()
    }

    fn check_link(&self, link: &Link, problems: &mut Vec<Problem>) {
        match link {
            Link::Timeline { id, chapter } => {
                let Some(timeline) = self.timelines.get(id) else {
                    problems.push(error(
                        "missingTarget",
                        format!("link targets missing timeline {}", id.0),
                    ));
                    return;
                };
                let chapter_index = usize::try_from(*chapter).unwrap_or(usize::MAX);
                if chapter_index >= timeline.chapters.len() {
                    problems.push(error(
                        "missingTarget",
                        format!("timeline {} has no chapter {}", id.0, chapter),
                    ));
                }
            }
            Link::Menu { id, .. } => {
                if !self.menus.contains_key(id) {
                    problems.push(error(
                        "missingTarget",
                        format!("link targets missing menu {}", id.0),
                    ));
                }
            }
            Link::Playlist { id } => {
                if !self.playlists.contains_key(id) {
                    problems.push(error(
                        "missingTarget",
                        format!("link targets missing playlist {id}"),
                    ));
                }
            }
            Link::ChapterPlaylist { id } => {
                if !self.chapter_playlists.contains_key(id) {
                    problems.push(error(
                        "missingTarget",
                        format!("link targets missing chapter playlist {id}"),
                    ));
                }
            }
            Link::Slideshow { id } => {
                if !self.slideshows.contains_key(id) {
                    problems.push(error(
                        "missingTarget",
                        format!("link targets missing slideshow {id}"),
                    ));
                }
            }
            Link::NotSet | Link::Stop | Link::LastMenu => {}
        }
    }

    fn check_dvd_limits(&self, problems: &mut Vec<Problem>) {
        if self.settings.kind != DiscKind::Dvd {
            return;
        }

        if self.timelines.len() > 99 {
            problems.push(error(
                "tooManyTitles",
                "DVD supports at most 99 titles".to_string(),
            ));
        }

        for timeline in self.timelines.values() {
            if timeline.chapters.len() > 99 {
                problems.push(error(
                    "tooManyChapters",
                    format!("timeline {} has more than 99 chapters", timeline.id.0),
                ));
            }
            if timeline.audio.len() > 8 {
                problems.push(error(
                    "tooManyAudio",
                    format!("timeline {} has more than 8 audio tracks", timeline.id.0),
                ));
            }
            if timeline.subtitles.len() > 32 {
                problems.push(error(
                    "tooManySubtitles",
                    format!(
                        "timeline {} has more than 32 subtitle tracks",
                        timeline.id.0
                    ),
                ));
            }
        }

        for region in &self.settings.regions {
            if !(1..=8).contains(region) {
                problems.push(error(
                    "badRegion",
                    format!("DVD region {} is outside 1..=8", region),
                ));
            }
        }
    }

    fn check_warnings(&self, problems: &mut Vec<Problem>) {
        if self.settings.first_play == Link::NotSet {
            problems.push(warning(
                "firstPlayNotSet",
                "first play is not set".to_string(),
            ));
        }

        for timeline in self.timelines.values() {
            if timeline.end_action == Link::NotSet {
                problems.push(warning(
                    "endActionNotSet",
                    format!("timeline {} end action is not set", timeline.id.0),
                ));
            }
        }

        for asset in self.assets.values() {
            if asset.offline {
                problems.push(warning(
                    "assetOffline",
                    format!("asset {} is offline", asset.id.0),
                ));
            }
        }

        // User operations are written into DVD PGCs; a Blu-ray build leaves them out for now.
        let restricted = self
            .timelines
            .values()
            .any(|timeline| !timeline.prohibited_ops.is_empty())
            || self
                .menus
                .values()
                .any(|menu| !menu.prohibited_ops.is_empty())
            || !self.settings.prohibited_ops.is_empty();
        if restricted && self.settings.kind == crate::model::DiscKind::BluRay {
            problems.push(warning(
                "bluRayUserOps",
                "Blu-ray discs do not apply user operations yet (they are DVD only)".to_string(),
            ));
        }
        let selects_streams = self
            .menus
            .values()
            .any(|menu| menu.buttons.iter().any(|button| button.selects_streams()));
        if selects_streams && self.settings.kind == crate::model::DiscKind::BluRay {
            problems.push(warning(
                "bluRayButtonStreams",
                "Blu-ray discs do not select audio or subtitles from buttons yet (DVD only)"
                    .to_string(),
            ));
        }
        let overridden = self.settings.first_play_override.is_some()
            || self
                .timelines
                .values()
                .any(|timeline| timeline.override_link.is_some())
            || self
                .chapter_playlists
                .values()
                .any(|playlist| playlist.override_link.is_some())
            || self.menus.values().any(|menu| {
                menu.override_link.is_some()
                    || menu
                        .buttons
                        .iter()
                        .any(|button| button.override_link.is_some())
            });
        if overridden && self.settings.kind == crate::model::DiscKind::BluRay {
            problems.push(warning(
                "bluRayOverride",
                "Blu-ray discs do not apply overrides yet (they are DVD only)".to_string(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn user_ops_on_blu_ray_are_a_warning() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = crate::Project::default();
        let menu = project.add_menu(None)?;
        if let Some(menu) = project.menu_mut(menu) {
            menu.prohibited_ops = vec![crate::UserOp::ButtonSelect];
        }
        let has = |project: &crate::Project| {
            project
                .check()
                .iter()
                .any(|problem| problem.code == "bluRayUserOps")
        };
        assert!(!has(&project));
        project.settings.kind = crate::DiscKind::BluRay;
        assert!(has(&project));
        Ok(())
    }
    use super::{Problem, Severity, has_errors};
    use crate::model::{Chapter, Project};
    use crate::{AssetId, DiscKind, Link, Tick, TimelineId};
    use std::error::Error;
    use std::path::Path;

    fn valid_project() -> Result<Project, Box<dyn Error>> {
        let mut p = Project::default();
        let asset = p.add_asset(Path::new("movie.mp4"))?;
        let tl = p.add_timeline(asset, None)?;
        p.add_chapter(tl, Tick(100), None)?;
        Ok(p)
    }

    fn has_code(problems: &[Problem], code: &str) -> bool {
        problems.iter().any(|p| p.code == code)
    }

    fn has_warning_code(problems: &[Problem], code: &str) -> bool {
        problems
            .iter()
            .any(|p| p.code == code && p.severity == Severity::Warning)
    }

    #[test]
    fn valid_has_no_errors() -> Result<(), Box<dyn Error>> {
        let p = valid_project()?;
        let problems = p.check();
        assert!(!has_errors(&problems));
        Ok(())
    }

    #[test]
    fn id_mismatch() -> Result<(), Box<dyn Error>> {
        let mut p = valid_project()?;
        let asset = p.assets.get(&AssetId(1)).cloned().ok_or("missing asset")?;
        let _ = p.assets.remove(&AssetId(1));
        p.assets.insert(AssetId(5), asset);
        let problems = p.check();
        assert!(has_code(&problems, "idMismatch"));
        Ok(())
    }

    #[test]
    fn next_id() -> Result<(), Box<dyn Error>> {
        let mut p = valid_project()?;
        p.next_id = 1;
        let problems = p.check();
        assert!(has_code(&problems, "nextId"));
        Ok(())
    }

    #[test]
    fn missing_asset() -> Result<(), Box<dyn Error>> {
        let mut p = valid_project()?;
        let _ = p.assets.remove(&AssetId(1));
        let problems = p.check();
        assert!(has_code(&problems, "missingAsset"));
        Ok(())
    }

    #[test]
    fn first_chapter() -> Result<(), Box<dyn Error>> {
        let mut p = valid_project()?;
        let tl = p.timeline_mut(TimelineId(2)).ok_or("missing timeline")?;
        tl.chapters.clear();
        let problems = p.check();
        assert!(has_code(&problems, "firstChapter"));
        Ok(())
    }

    #[test]
    fn chapter_order() -> Result<(), Box<dyn Error>> {
        let mut p = valid_project()?;
        let tl = p.timeline_mut(TimelineId(2)).ok_or("missing timeline")?;
        tl.chapters = vec![
            Chapter {
                time: Tick::ZERO,
                name: "Chapter 1".to_string(),
                poster: None,
            },
            Chapter {
                time: Tick(100),
                name: "Chapter 2".to_string(),
                poster: None,
            },
            Chapter {
                time: Tick(50),
                name: "Chapter 3".to_string(),
                poster: None,
            },
        ];
        let problems = p.check();
        assert!(has_code(&problems, "chapterOrder"));
        Ok(())
    }

    #[test]
    fn missing_target() -> Result<(), Box<dyn Error>> {
        let mut p = valid_project()?;
        p.settings.first_play = Link::Timeline {
            id: TimelineId(99),
            chapter: 0,
        };
        let problems = p.check();
        assert!(has_code(&problems, "missingTarget"));
        Ok(())
    }

    #[test]
    fn too_many_chapters() -> Result<(), Box<dyn Error>> {
        let mut p = valid_project()?;
        p.settings.kind = DiscKind::Dvd;
        let tl = p.timeline_mut(TimelineId(2)).ok_or("missing timeline")?;
        tl.chapters = (0..100)
            .map(|i| Chapter {
                time: Tick(i64::from(i)),
                name: format!("Chapter {}", i),
                poster: None,
            })
            .collect();
        let problems = p.check();
        assert!(has_code(&problems, "tooManyChapters"));
        Ok(())
    }

    #[test]
    fn bad_region() -> Result<(), Box<dyn Error>> {
        let mut p = valid_project()?;
        p.settings.regions = vec![9];
        let problems = p.check();
        assert!(has_code(&problems, "badRegion"));
        Ok(())
    }

    #[test]
    fn first_play_not_set_is_warning() -> Result<(), Box<dyn Error>> {
        let p = valid_project()?;
        let problems = p.check();
        assert!(has_warning_code(&problems, "firstPlayNotSet"));
        assert!(!has_errors(&problems));
        Ok(())
    }
}
