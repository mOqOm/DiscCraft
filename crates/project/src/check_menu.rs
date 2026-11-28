//! Menu validation.

use crate::check::{Problem, Severity};
use crate::ids::{MenuId, TimelineId};
use crate::link::Link;
use crate::menu::{
    LETTERBOX_MOST_BUTTONS, MAX_BUTTONS, MAX_MENU_SECONDS, Menu, MenuBackground, Rect, frame_size,
};
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
    check_letterbox_buttons(project, problems);
}

/// A 16:9 DVD menu has its highlights for a 4:3 television's letterboxed picture in a second button group, of 18
/// buttons at most (Encore turns the buttons past 18 into objects: 「メニューにボタンが 18 個以上あります。いくつかの
/// ボタンはオブジェクトに変換されます。」). DiscCraft keeps them all in one group and warns: there the highlights
/// are not on the letterboxed buttons.
fn check_letterbox_buttons(project: &Project, problems: &mut Vec<Problem>) {
    if project.settings.kind != crate::model::DiscKind::Dvd || !project.menus_wide() {
        return;
    }
    for menu in project.menus.values() {
        if menu.buttons.len() > LETTERBOX_MOST_BUTTONS {
            problems.push(warning(
                "wideMenuButtons",
                format!(
                    "menu {} has more than {LETTERBOX_MOST_BUTTONS} buttons at 16:9; a 4:3 television shows its                      highlights off the letterboxed buttons",
                    menu.id.0
                ),
            ));
        }
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
    check_resume(project, menu, problems);
}

/// M6.529: a menu's Resume (a button's, or its end action) goes back to a title only when the menu is shown from
/// one. On a DVD that is a title's Menu button (its menu remote: this menu, or the last menu) or the Title button;
/// on a Blu-ray, a pop-up over the title, or the menu the Top Menu key opens (the Title button's, else the first
/// menu). Shown any other way, Resume does nothing there.
fn check_resume(project: &Project, menu: &Menu, problems: &mut Vec<Problem>) {
    let resumes = menu.end_action == Link::Resume
        || menu
            .buttons
            .iter()
            .any(|button| button.link == Link::Resume);
    if !resumes {
        return;
    }
    let root = project.menu_root(menu.id);
    let shows_this =
        |link: &Link| matches!(link, Link::Menu { id, .. } if project.menu_root(*id) == root);
    let reached = if project.settings.kind == crate::model::DiscKind::BluRay {
        let title_button = &project.settings.title_remote;
        let top_menu = if matches!(title_button, Link::Menu { .. }) {
            shows_this(title_button)
        } else {
            project
                .menus
                .keys()
                .next()
                .is_some_and(|first| project.menu_root(*first) == root)
        };
        menu.popup || top_menu
    } else {
        shows_this(&project.settings.title_remote)
            || project.timelines.values().any(|timeline| {
                shows_this(&timeline.menu_remote) || timeline.menu_remote == Link::LastMenu
            })
    };
    if !reached {
        problems.push(warning(
            "resumeUnreachable",
            format!(
                "menu {} has Resume, but no title's Menu button or the Title button shows it: Resume does nothing there",
                menu.id.0
            ),
        ));
    }
}

/// Menu audio must be an audio asset, or (M6.430) the video of the menu's motion background, whose sound it is (a
/// Blu-ray menu's video carries its music).
fn check_audio(project: &Project, menu: &Menu, problems: &mut Vec<Problem>) {
    if let Some(asset) = menu.audio {
        let background_video =
            matches!(menu.background, MenuBackground::Video { asset: video, .. } if video == asset);
        let valid = background_video
            || project
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
    if let MenuBackground::Video { asset, seconds, .. } = &menu.background {
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
    // A still background, or the PSD over a motion video (M6.402).
    if let Some(asset) = &menu.background.document() {
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

/// Overlapping buttons: editing may make them (as in Encore), the check reports them and a DVD build refuses them
/// (a disc's button areas must not overlap).
fn check_overlaps(menu: &Menu, problems: &mut Vec<Problem>) {
    if has_overlapping_buttons(menu) {
        problems.push(error(
            "buttonsOverlap",
            format!("menu {} has overlapping buttons", menu.id.0),
        ));
    }
}

/// Whether two of `menu`'s buttons share any area.
pub fn has_overlapping_buttons(menu: &Menu) -> bool {
    menu.buttons.iter().enumerate().any(|(index, first)| {
        menu.buttons
            .iter()
            .skip(index.saturating_add(1))
            .any(|second| rects_overlap(&first.rect, &second.rect))
    })
}

/// For each button of `menu`, whether it overlaps another one (Encore outlines those in red in the Menu Viewer).
pub fn overlapping_buttons(menu: &Menu) -> Vec<bool> {
    menu.buttons
        .iter()
        .enumerate()
        .map(|(index, button)| {
            menu.buttons
                .iter()
                .enumerate()
                .any(|(other, second)| other != index && rects_overlap(&button.rect, &second.rect))
        })
        .collect()
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
        Link::NotSet | Link::Stop | Link::LastMenu | Link::Resume => {}
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

    /// M6.529: Resume on a DVD menu that no title's Menu button nor the Title button shows does nothing: a warning,
    /// gone once a title's menu remote shows it. On a Blu-ray a pop-up resumes, and so does the menu the Top Menu
    /// key opens (with no Title button menu, the first).
    #[test]
    fn resume_needs_a_menu_shown_from_a_title() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let asset = project.add_asset(Path::new("movie.mp4"))?;
        let timeline = project.add_timeline(asset, None)?;
        let first = project.add_menu(Some("Main".to_string()))?;
        project.add_button(
            first,
            "Play".to_string(),
            None,
            Link::Timeline {
                id: timeline,
                chapter: 0,
            },
        )?;
        let menu = project.add_menu(Some("Pause".to_string()))?;
        project.add_button(menu, "Back".to_string(), None, Link::Resume)?;
        if let Some(found) = project.timeline_mut(timeline) {
            found.menu_remote = Link::NotSet;
        }
        assert!(has_warning_code(
            &problems_for(&project),
            "resumeUnreachable"
        ));
        if let Some(found) = project.timeline_mut(timeline) {
            found.menu_remote = Link::Menu {
                id: menu,
                button: None,
            };
        }
        assert!(!has_warning_code(
            &problems_for(&project),
            "resumeUnreachable"
        ));

        project.settings.kind = crate::model::DiscKind::BluRay;
        assert!(has_warning_code(
            &problems_for(&project),
            "resumeUnreachable"
        ));
        if let Some(found) = project.menu_mut(menu) {
            found.popup = true;
        }
        assert!(!has_warning_code(
            &problems_for(&project),
            "resumeUnreachable"
        ));
        if let Some(found) = project.menu_mut(menu) {
            found.popup = false;
        }
        project.settings.title_remote = Link::Menu {
            id: menu,
            button: None,
        };
        assert!(!has_warning_code(
            &problems_for(&project),
            "resumeUnreachable"
        ));
        Ok(())
    }

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
                    effects: Default::default(),
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
                    label_offset: [0, 0],
                    thumbnail_offset: [0, 0],
                    body: None,
                    psd_layer: None,
                    hidden: false,
                    locked: false,
                });
            }
        }

        let problems = problems_for(&project);
        assert!(has_error_code(&problems, "tooManyButtons"));

        Ok(())
    }

    /// M6.437: a 16:9 DVD menu of more than 18 buttons has no letterbox button group (Encore keeps 18); a 4:3 menu
    /// and a Blu-ray menu have none to have.
    #[test]
    fn a_wide_dvd_menu_of_more_than_18_buttons_warns() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        for index in 0..18u16 {
            let rect = Rect {
                x: 40 + (index % 2) * 320,
                y: 20 + (index / 2) * 40,
                w: 280,
                h: 30,
            };
            project.add_button(menu, format!("B{index}"), Some(rect), Link::Stop)?;
        }
        project.menu_mut(menu).ok_or("menu")?.widescreen = true;
        let warns = |project: &Project| has_warning_code(&problems_for(project), "wideMenuButtons");
        assert!(!warns(&project));
        let rect = Rect {
            x: 40,
            y: 400,
            w: 280,
            h: 30,
        };
        project.add_button(menu, "B18".to_owned(), Some(rect), Link::Stop)?;
        assert!(warns(&project));
        project.menu_mut(menu).ok_or("menu")?.widescreen = false;
        assert!(!warns(&project));
        project.menu_mut(menu).ok_or("menu")?.widescreen = true;
        project.settings.kind = crate::model::DiscKind::BluRay;
        assert!(!warns(&project));
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

        // Each overlapping button is told apart (the Menu Viewer outlines them in red); one clear of the others
        // is not.
        let apart = Rect {
            x: 300,
            y: 200,
            w: 100,
            h: 20,
        };
        project.add_button(menu, "C".to_string(), Some(apart), Link::Stop)?;
        let menu = project.menu(menu).ok_or("missing menu")?;
        assert_eq!(super::overlapping_buttons(menu), vec![true, true, false]);

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
