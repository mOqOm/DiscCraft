//! M10.10 check: a built `BDMV` folder runs in the preview's HDMV interpreter as its project says: the first play
//! menu, its buttons to the title and to a chapter, the title's end action back to the menu with the button a link
//! asked for, the Top Menu key, and the title's pop-up menu. Skipped when ffmpeg (which makes the video) is not
//! available.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_bd::build_bdmv;
use disccraft_project::{DiscKind, Link, Project, Tick};
use disccraft_vm::{BdLocation, BdVm, Key, load_bdmv};
use std::path::PathBuf;
use std::process::Command;

const TICKS_PER_SECOND: i64 = 254_016_000_000;

fn ffmpeg() -> Option<PathBuf> {
    let path =
        std::env::var_os("DISCCRAFT_FFMPEG").map_or_else(|| PathBuf::from("ffmpeg"), PathBuf::from);
    Command::new(&path)
        .arg("-version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| path)
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures")
}

/// Six seconds of DVD-compliant MPEG-2 (passed through).
fn video() -> Option<PathBuf> {
    let video = fixtures().join("bd_preview.m2v");
    if video.is_file() {
        return Some(video);
    }
    let ffmpeg = ffmpeg()?;
    std::fs::create_dir_all(fixtures()).unwrap();
    let ok = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .args(["testsrc=size=720x480:rate=30000/1001", "-t", "6"])
        .args(["-vf", "setdar=4/3", "-c:v", "mpeg2video", "-b:v", "1500k"])
        .args([
            "-maxrate", "6M", "-bufsize", "1835k", "-g", "15", "-bf", "0",
        ])
        .args(["-pix_fmt", "yuv420p", "-f", "mpeg2video"])
        .arg(&video)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ffmpeg could not create the fixture");
    Some(video)
}

#[test]
fn a_built_disc_navigates_in_the_preview_interpreter() {
    let Some(video) = video() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    let v = p.add_asset(&video).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    p.add_chapter(t, Tick(3 * TICKS_PER_SECOND), None).unwrap();
    let menu = p.add_menu(Some("Top".to_string())).unwrap();
    p.menu_mut(menu).unwrap().buttons.clear();
    p.add_button(
        menu,
        "Play".to_string(),
        None,
        Link::Timeline { id: t, chapter: 0 },
    )
    .unwrap();
    p.add_button(
        menu,
        "Chapter 2".to_string(),
        None,
        Link::Timeline { id: t, chapter: 1 },
    )
    .unwrap();
    // The title goes back to the menu, on its second button.
    p.timeline_mut(t).unwrap().end_action = Link::Menu {
        id: menu,
        button: Some(1),
    };
    // Its pop-up menu: one button back to the top menu.
    let popup = p.add_menu(Some("Popup".to_string())).unwrap();
    p.menu_mut(popup).unwrap().buttons.clear();
    p.add_button(
        popup,
        "Menu".to_string(),
        None,
        Link::Menu {
            id: menu,
            button: None,
        },
    )
    .unwrap();
    p.menu_mut(popup).unwrap().popup = true;
    p.timeline_mut(t).unwrap().menu_remote = Link::Menu {
        id: popup,
        button: None,
    };
    p.settings.first_play = Link::Menu {
        id: menu,
        button: None,
    };
    p.settings.title_remote = Link::Menu {
        id: menu,
        button: None,
    };

    let out = fixtures().join("bd_preview_out");
    let _ = build_bdmv(&p, &out).unwrap();
    let disc = load_bdmv(&out).unwrap();

    // The first play: the top menu's playlist, its first button selected.
    let mut vm = BdVm::start(&disc).unwrap();
    let BdLocation::Playing {
        playlist: menu_playlist,
        mark: 0,
    } = vm.state().location
    else {
        panic!("not in the menu: {:?}", vm.state().location);
    };
    let (shown, view) = vm.menu().unwrap();
    assert!(!shown.popup);
    let first = view.button;
    assert_eq!(shown.pages.first().unwrap().buttons.len(), 2);

    // Down and Enter: the second button plays the title from its second chapter.
    vm.press(Key::Down).unwrap();
    assert_ne!(vm.menu().unwrap().1.button, first);
    vm.press(Key::Enter).unwrap();
    assert_eq!(
        vm.state().location,
        BdLocation::Playing {
            playlist: 1,
            mark: 1
        }
    );

    // The pop-up over the title, and its button back to the menu.
    assert!(vm.has_popup());
    vm.popup_key();
    assert!(
        vm.menu()
            .is_some_and(|(menu, view)| menu.popup && view.open)
    );
    vm.press(Key::Enter).unwrap();
    assert_eq!(
        vm.state().location,
        BdLocation::Playing {
            playlist: menu_playlist,
            mark: 0
        }
    );

    // Play from the start; the end action goes back to the menu on its second button.
    let (_, view) = vm.menu().unwrap();
    if view.button != first {
        vm.press(Key::Up).unwrap();
    }
    vm.press(Key::Enter).unwrap();
    assert_eq!(
        vm.state().location,
        BdLocation::Playing {
            playlist: 1,
            mark: 0
        }
    );
    vm.finish().unwrap();
    assert_eq!(
        vm.state().location,
        BdLocation::Playing {
            playlist: menu_playlist,
            mark: 0
        }
    );
    assert_ne!(vm.menu().unwrap().1.button, first);

    // The Top Menu key from the title.
    vm.press(Key::Enter).unwrap();
    vm.top_menu().unwrap();
    assert_eq!(
        vm.state().location,
        BdLocation::Playing {
            playlist: menu_playlist,
            mark: 0
        }
    );
}

/// M10.26 (GitHub issue #13, as Encore builds a menu's pages): a menu's two pages are one clip whose IG has them as
/// pages 1 and 2 after page 0's auto-action button. The disc starts on the first page; its Next button turns to
/// the second page on the same play of the menu's playlist; Back turns back; the title's end action to the second
/// page's button lands there. Each page's drawing is in its buttons' states where they are (M10.28).
#[test]
fn a_menus_pages_turn_in_one_composition() {
    let Some(video) = video() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    let v = p.add_asset(&video).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    p.add_chapter(t, Tick(3 * TICKS_PER_SECOND), None).unwrap();
    let menu = p.add_menu(Some("Top".to_string())).unwrap();
    p.menu_mut(menu).unwrap().buttons.clear();
    let page = p.add_menu_page(menu).unwrap();
    p.add_button(
        menu,
        "Play".to_string(),
        None,
        Link::Timeline { id: t, chapter: 0 },
    )
    .unwrap();
    p.add_button(
        menu,
        "Next".to_string(),
        None,
        Link::Menu {
            id: page,
            button: Some(1),
        },
    )
    .unwrap();
    p.add_button(
        page,
        "Back".to_string(),
        None,
        Link::Menu {
            id: menu,
            button: Some(1),
        },
    )
    .unwrap();
    p.add_button(
        page,
        "Chapter 2".to_string(),
        None,
        Link::Timeline { id: t, chapter: 1 },
    )
    .unwrap();
    p.timeline_mut(t).unwrap().end_action = Link::Menu {
        id: page,
        button: Some(1),
    };
    p.settings.first_play = Link::Menu {
        id: menu,
        button: None,
    };

    let out = fixtures().join("bd_pages_out");
    let _ = std::fs::remove_dir_all(&out);
    let _ = build_bdmv(&p, &out).unwrap();
    // The title's clip and one clip for both pages.
    let clips = std::fs::read_dir(out.join("BDMV/STREAM")).unwrap().count();
    assert_eq!(clips, 2);
    let disc = load_bdmv(&out).unwrap();

    // The first page, its default button.
    let mut vm = BdVm::start(&disc).unwrap();
    let BdLocation::Playing {
        playlist: menu_playlist,
        mark: 0,
    } = vm.state().location
    else {
        panic!("not in the menu: {:?}", vm.state().location);
    };
    let on_menu = BdLocation::Playing {
        playlist: menu_playlist,
        mark: 0,
    };
    let (shown, view) = vm.menu().unwrap();
    assert_eq!(shown.pages.len(), 3);
    let zero = shown.pages.first().unwrap();
    assert_eq!(zero.buttons.len(), 1);
    assert!(zero.buttons.first().unwrap().auto_action);
    // M10.28: each page's buttons carry their texts in their normal state (the page's picture under them), and
    // nothing else is drawn on these pages (no picture beside the buttons).
    for page in shown.pages.iter().skip(1) {
        assert!(
            page.buttons
                .iter()
                .all(|button| button.id < disccraft_bd::PAGE_PICTURE_BUTTON
                    && button.normal_object != 0xFFFF),
            "page {}: {:?}",
            page.id,
            page.buttons
        );
    }
    assert_eq!((view.page, view.button), (1, 0));

    // Next: the second page's Chapter 2, the same play of the menu.
    vm.press(Key::Down).unwrap();
    assert_eq!(vm.menu().unwrap().1.button, 1);
    vm.press(Key::Enter).unwrap();
    assert_eq!(vm.state().location, on_menu);
    let view = vm.menu().unwrap().1;
    assert_eq!((view.page, view.button), (2, 1));

    // Back: the first page's Next.
    vm.press(Key::Up).unwrap();
    vm.press(Key::Enter).unwrap();
    assert_eq!(vm.state().location, on_menu);
    let view = vm.menu().unwrap().1;
    assert_eq!((view.page, view.button), (1, 1));

    // Next, then Chapter 2 plays the title; its end comes back to the second page's Chapter 2.
    vm.press(Key::Enter).unwrap();
    vm.press(Key::Enter).unwrap();
    assert_eq!(
        vm.state().location,
        BdLocation::Playing {
            playlist: 1,
            mark: 1
        }
    );
    vm.finish().unwrap();
    assert_eq!(vm.state().location, on_menu);
    let view = vm.menu().unwrap().1;
    assert_eq!((view.page, view.button), (2, 1));
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&out);
    }
}
