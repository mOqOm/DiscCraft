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
