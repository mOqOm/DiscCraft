//! Menu validation.

use crate::check::{Problem, Severity};
use crate::ids::{MenuId, TimelineId};
use crate::link::Link;
use crate::menu::{MAX_BUTTONS, MAX_MENU_SECONDS, Menu, MenuBackground, Rect, frame_size};
use crate::model::{AssetKind, Project};

/// Validates menus and buttons.
pub(crate) fn check_menus(project: &Project, problems: &mut Vec<Problem>) {
    for menu in project.menus.values() {
        check_menu(project, menu, problems);
    }
    // A disc's menus share one video attribute, so they all come out at the first menu's aspect ratio.
    let wide = project.menus_wide();
    if project.menus.values().any(|menu| menu.widescreen != wide) {
        problems.push(warning(
            "menuAspectMix",
            format!(
                "menus mix 4:3 and 16:9; the disc shows every menu as {}",
                if wide { "16:9" } else { "4:3" }
            ),
        ));
    }
}

fn check_menu(project: &Project, menu: &Menu, problems: &mut Vec<Problem>) {
    check_background(project, menu, problems);
    check_graphics(project, menu, problems);
    check_button_count(menu, problems);
    check_button_rects(project, menu, problems);
    check_default_button(menu, problems);
    check_buttons(project, menu, problems);
    check_duration(menu, problems);
    check_audio(project, menu, problems);
}

/// Menu audio must be an audio asset.
fn check_audio(project: &Project, menu: &Menu, problems: &mut Vec<Problem>) {
    if let Some(asset) = menu.audio {
        let valid = project
            .assets
            .get(&asset)
            .is_some_and(|asset| asset.kind == AssetKind::Audio);
        if !valid {
            problems.push(error(
                "missingAsset",
                format!("menu {} audio needs an audio asset", menu.id.0),
            ));
        }
    }
}

/// A still menu's duration must fit a DVD cell's still time (1 to 254 seconds).
fn check_duration(menu: &Menu, problems: &mut Vec<Problem>) {
    if let Some(seconds) = menu.duration_seconds
        && !(1..=MAX_MENU_SECONDS).contains(&seconds)
    {
        problems.push(error(
            "menuDuration",
            format!(
                "menu {} must stay between 1 and {MAX_MENU_SECONDS} seconds",
                menu.id.0
            ),
        ));
    }
}

/// Graphic layers: an image must be an image asset of the project; every rectangle must lie in the frame.
fn check_graphics(project: &Project, menu: &Menu, problems: &mut Vec<Problem>) {
    let frame = crate::menu::frame_size(&project.settings);
    for (index, graphic) in menu.graphics.iter().enumerate() {
        if let crate::graphic::GraphicKind::Image { asset } = graphic.kind {
            let valid = project
                .assets
                .get(&asset)
                .is_some_and(|asset| asset.kind == AssetKind::Image);
            if !valid {
                problems.push(error(
                    "missingAsset",
                    format!(
                        "menu {} graphic {index} asset {} is missing or not an image",
                        menu.id.0, asset.0
                    ),
                ));
            }
        }
        if crate::graphic::checked_graphic_rect(graphic.rect, frame).is_err() {
            problems.push(error(
                "graphicOutOfFrame",
                format!("menu {} graphic {index} lies outside the frame", menu.id.0),
            ));
        }
    }
}

fn check_background(project: &Project, menu: &Menu, problems: &mut Vec<Problem>) {
    if let MenuBackground::Video { asset, seconds } = &menu.background {
        let valid = project
            .assets
            .get(asset)
            .is_some_and(|a| a.kind == AssetKind::Video);
        if !valid || !(1..=30).contains(seconds) {
            problems.push(error(
                "missingAsset",
                format!(
                    "menu {} motion background needs a video asset and 1–30 seconds",
                    menu.id.0
                ),
            ));
        }
    }
    if let MenuBackground::Image { asset } = &menu.background {
        let valid = project
            .assets
            .get(asset)
            .is_some_and(|asset| asset.kind == AssetKind::Image);

        if !valid {
            problems.push(error(
                "missingAsset",
                format!(
                    "menu {} background asset {} is missing or not an image",
                    menu.id.0, asset.0
                ),
            ));
        }
    }
}

fn check_button_count(menu: &Menu, problems: &mut Vec<Problem>) {
    if menu.buttons.len() > MAX_BUTTONS {
        problems.push(error(
            "tooManyButtons",
            format!("menu {} has more than {} buttons", menu.id.0, MAX_BUTTONS),
        ));
    }
}

fn check_button_rects(project: &Project, menu: &Menu, problems: &mut Vec<Problem>) {
    let (frame_w, frame_h) = frame_size(&project.settings);

    for (index, button) in menu.buttons.iter().enumerate() {
        if !rect_in_frame(&button.rect, frame_w, frame_h) {
            problems.push(error(
                "buttonOutOfFrame",
                format!("menu {} button {} is out of frame", menu.id.0, index),
            ));
        }
    }

    check_overlaps(menu, problems);
}

fn rect_in_frame(rect: &Rect, frame_w: u16, frame_h: u16) -> bool {
    if rect.w < 8 || rect.h < 8 {
        return false;
    }

    let right = u32::from(rect.x).checked_add(u32::from(rect.w));
    let bottom = u32::from(rect.y).checked_add(u32::from(rect.h));

    right.is_some_and(|right| right <= u32::from(frame_w))
        && bottom.is_some_and(|bottom| bottom <= u32::from(frame_h))
}

fn check_overlaps(menu: &Menu, problems: &mut Vec<Problem>) {
    for (index, first) in menu.buttons.iter().enumerate() {
        for second in menu.buttons.iter().skip(index.saturating_add(1)) {
            if rects_overlap(&first.rect, &second.rect) {
                problems.push(error(
                    "buttonsOverlap",
                    format!("menu {} has overlapping buttons", menu.id.0),
                ));
                return;
            }
        }
    }
}

/// Whether two rectangles share any area (touching edges is fine).
pub(crate) fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    let a_right = u32::from(a.x).saturating_add(u32::from(a.w));
    let b_right = u32::from(b.x).saturating_add(u32::from(b.w));
    let a_bottom = u32::from(a.y).saturating_add(u32::from(a.h));
    let b_bottom = u32::from(b.y).saturating_add(u32::from(b.h));

    u32::from(a.x) < b_right
        && u32::from(b.x) < a_right
        && u32::from(a.y) < b_bottom
        && u32::from(b.y) < a_bottom
}

fn check_default_button(menu: &Menu, problems: &mut Vec<Problem>) {
    if menu.buttons.is_empty() {
        return;
    }

    let len = u32::try_from(menu.buttons.len()).unwrap_or(u32::MAX);
    if menu.default_button >= len {
        problems.push(error(
            "badDefaultButton",
            format!(
                "menu {} default button {} is out of range",
                menu.id.0, menu.default_button
            ),
        ));
    }
}

fn check_buttons(project: &Project, menu: &Menu, problems: &mut Vec<Problem>) {
    if menu.buttons.is_empty() {
        problems.push(warning(
            "menuWithoutButtons",
            format!("menu {} has no buttons", menu.id.0),
        ));
    }

    for (index, button) in menu.buttons.iter().enumerate() {
        if button.link == Link::NotSet {
            problems.push(warning(
                "buttonLinkNotSet",
                format!("menu {} button {} has no link", menu.id.0, index),
            ));
        }

        check_button_link(project, menu, index, &button.link, problems);
    }
}

fn check_button_link(
    project: &Project,
    menu: &Menu,
    index: usize,
    link: &Link,
    problems: &mut Vec<Problem>,
) {
    match link {
        Link::Timeline { id, chapter } => {
            check_timeline_target(project, menu, index, *id, *chapter, problems);
        }
        Link::Menu { id, button } => {
            check_menu_target(project, menu, index, *id, *button, problems);
        }
        Link::Playlist { id } => {
            if !project.playlists.contains_key(id) {
                push_missing_target(
                    menu,
                    index,
                    format!("playlist {id} does not exist"),
                    problems,
                );
            }
        }
        Link::ChapterPlaylist { id } => {
            if !project.chapter_playlists.contains_key(id) {
                push_missing_target(
                    menu,
                    index,
                    format!("chapter playlist {id} does not exist"),
                    problems,
                );
            }
        }
        Link::Slideshow { id } => {
            if !project.slideshows.contains_key(id) {
                push_missing_target(
                    menu,
                    index,
                    format!("slideshow {id} does not exist"),
                    problems,
                );
            }
        }
        Link::NotSet | Link::Stop | Link::LastMenu => {}
    }
}

fn check_timeline_target(
    project: &Project,
    menu: &Menu,
    index: usize,
    id: TimelineId,
    chapter: u32,
    problems: &mut Vec<Problem>,
) {
    let Some(timeline) = project.timelines.get(&id) else {
        push_missing_target(
            menu,
            index,
            format!("timeline {} does not exist", id.0),
            problems,
        );
        return;
    };

    let chapter_index = usize::try_from(chapter).unwrap_or(usize::MAX);
    if chapter_index >= timeline.chapters.len() {
        push_missing_target(
            menu,
            index,
            format!("timeline {} has no chapter {}", id.0, chapter),
            problems,
        );
    }
}

fn check_menu_target(
    project: &Project,
    menu: &Menu,
    index: usize,
    id: MenuId,
    button: Option<u32>,
    problems: &mut Vec<Problem>,
) {
    let Some(target) = project.menus.get(&id) else {
        push_missing_target(
            menu,
            index,
            format!("menu {} does not exist", id.0),
            problems,
        );
        return;
    };

    let Some(button_index) = button else {
        return;
    };

    let len = u32::try_from(target.buttons.len()).unwrap_or(u32::MAX);
    if button_index >= len {
        push_missing_target(
            menu,
            index,
            format!("menu {} has no button {}", id.0, button_index),
            problems,
        );
    }
}

fn push_missing_target(menu: &Menu, index: usize, detail: String, problems: &mut Vec<Problem>) {
    problems.push(error(
        "missingTarget",
        format!("menu {} button {} targets {detail}", menu.id.0, index),
    ));
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
    use super::check_menus;
    use crate::check::{Problem, Severity};
    use crate::ids::{AssetId, TimelineId};
    use crate::link::Link;
    use crate::menu::{Button, MAX_BUTTONS, MenuBackground, Rect};
    use crate::model::Project;
    use std::error::Error;
    use std::path::Path;

    fn problems_for(project: &Project) -> Vec<Problem> {
        let mut problems = Vec::new();
        check_menus(project, &mut problems);
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
    fn valid_menu_has_no_errors() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        project.add_button(menu, "Play".to_string(), None, Link::Stop)?;

        let problems = problems_for(&project);
        assert!(
            !problems
                .iter()
                .any(|problem| problem.severity == Severity::Error)
        );

        Ok(())
    }

    #[test]
    fn too_many_buttons() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;

        {
            let menu_obj = project.menu_mut(menu).ok_or("missing menu")?;
            for index in 0..=MAX_BUTTONS {
                menu_obj.buttons.push(Button {
                    name: format!("Button {index}"),
                    label: format!("Button {index}"),
                    rect: Rect {
                        x: 0,
                        y: 0,
                        w: 100,
                        h: 20,
                    },
                    link: Link::Stop,
                    auto_activate: false,
                    override_link: None,
                    highlight_group: 1,
                    sync_name: true,
                    name_from_link: false,
                    routes: None,
                    audio: None,
                    subtitle: crate::model::SubtitleDefault::NoChange,
                    kind: Default::default(),
                    text_subpicture: false,
                    transition: None,
                    label_format: Default::default(),
                    label_font_style: None,
                    shadow: None,
                    orientation: Default::default(),
                    thumbnail: None,
                    subpicture: None,
                });
            }
        }

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "tooManyButtons"));

        Ok(())
    }

    #[test]
    fn button_out_of_frame() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        project.add_button(
            menu,
            "Small".to_string(),
            Some(Rect {
                x: 0,
                y: 0,
                w: 4,
                h: 20,
            }),
            Link::Stop,
        )?;

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "buttonOutOfFrame"));

        Ok(())
    }

    #[test]
    fn buttons_overlap() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        let rect = Rect {
            x: 10,
            y: 10,
            w: 100,
            h: 20,
        };

        project.add_button(menu, "A".to_string(), Some(rect), Link::Stop)?;
        project.add_button(menu, "B".to_string(), Some(rect), Link::Stop)?;

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "buttonsOverlap"));

        Ok(())
    }

    #[test]
    fn menu_audio_must_be_an_audio_asset() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        let image = project.add_asset(Path::new("bg.png"))?;
        project.menu_mut(menu).ok_or("missing menu")?.audio = Some(image);
        assert!(has_error_code(&problems_for(&project), "missingAsset"));
        let song = project.add_asset(Path::new("song.wav"))?;
        project.menu_mut(menu).ok_or("missing menu")?.audio = Some(song);
        assert!(!has_error_code(&problems_for(&project), "missingAsset"));
        // The asset in use cannot be removed.
        assert!(project.asset_user(song).is_some());
        Ok(())
    }

    #[test]
    fn menu_duration_must_fit_a_cell_still_time() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        project
            .menu_mut(menu)
            .ok_or("missing menu")?
            .duration_seconds = Some(255);
        assert!(has_error_code(&problems_for(&project), "menuDuration"));
        project
            .menu_mut(menu)
            .ok_or("missing menu")?
            .duration_seconds = Some(30);
        assert!(!has_error_code(&problems_for(&project), "menuDuration"));
        Ok(())
    }

    #[test]
    fn bad_default_button() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        project.add_button(menu, "A".to_string(), None, Link::Stop)?;
        project.menu_mut(menu).ok_or("missing menu")?.default_button = 1;

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "badDefaultButton"));

        Ok(())
    }

    #[test]
    fn missing_timeline_target() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        project.add_button(
            menu,
            "T".to_string(),
            None,
            Link::Timeline {
                id: TimelineId(99),
                chapter: 0,
            },
        )?;

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "missingTarget"));

        Ok(())
    }

    #[test]
    fn missing_chapter_target() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        let asset = project.add_asset(Path::new("movie.mp4"))?;
        let timeline = project.add_timeline(asset, None)?;

        project.add_button(
            menu,
            "T".to_string(),
            None,
            Link::Timeline {
                id: timeline,
                chapter: 1,
            },
        )?;

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "missingTarget"));

        Ok(())
    }

    #[test]
    fn missing_menu_button_target() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        let target = project.add_menu(None)?;
        project.add_button(target, "Ok".to_string(), None, Link::Stop)?;

        project.add_button(
            menu,
            "Go".to_string(),
            None,
            Link::Menu {
                id: target,
                button: Some(1),
            },
        )?;

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "missingTarget"));

        Ok(())
    }

    #[test]
    fn background_missing_asset() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        project.menu_mut(menu).ok_or("missing menu")?.background =
            MenuBackground::Image { asset: AssetId(99) };

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "missingAsset"));

        Ok(())
    }

    #[test]
    fn background_not_image() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        let asset = project.add_asset(Path::new("movie.mp4"))?;
        project.menu_mut(menu).ok_or("missing menu")?.background = MenuBackground::Image { asset };

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "missingAsset"));

        Ok(())
    }

    #[test]
    fn background_valid_image() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        let asset = project.add_asset(Path::new("bg.png"))?;
        project.menu_mut(menu).ok_or("missing menu")?.background = MenuBackground::Image { asset };
        project.add_button(menu, "Play".to_string(), None, Link::Stop)?;

        let problems = problems_for(&project);
        assert!(!has_error_code(&problems, "missingAsset"));

        Ok(())
    }

    #[test]
    fn menu_without_buttons_warning() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.add_menu(None)?;

        let problems = problems_for(&project);
        assert!(has_warning_code(&problems, "menuWithoutButtons"));

        Ok(())
    }

    #[test]
    fn button_link_not_set_warning() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        project.add_button(menu, "No link".to_string(), None, Link::NotSet)?;

        let problems = problems_for(&project);
        assert!(has_warning_code(&problems, "buttonLinkNotSet"));

        Ok(())
    }
}
