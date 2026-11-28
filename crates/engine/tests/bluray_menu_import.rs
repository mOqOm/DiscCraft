//! M6.430 check: the menu of a Blu-ray DiscCraft builds (two buttons to two chapters, and a third to another title)
//! is imported into a DVD project as a menu with the same buttons, at the same places, highlighted by the shapes of
//! their selected pictures, linked to the same chapters of timelines of the same playlists (the main one, imported
//! before, used again). The video is made with ffmpeg; without it the test is skipped.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, build};
use disccraft_engine::Session;
use disccraft_project::{DiscKind, Link, MenuBackground, Project, Rect, Tick, TimelineId};
use disccraft_test_support::ffmpeg;
use serde_json::json;
use std::path::Path;
use std::process::Command;

/// Ticks of one NTSC frame.
const FRAME: i64 = 8_475_667_200;

fn video(ffmpeg: &Path, out: &Path, seconds: u32) {
    let ok = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc=size=720x480:rate=30000/1001")
        .args(["-t", &seconds.to_string(), "-vf", "setdar=16/9"])
        .args([
            "-c:v",
            "mpeg2video",
            "-b:v",
            "1500k",
            "-maxrate",
            "6M",
            "-bufsize",
            "1835k",
        ])
        // Closed GOPs: the MPEG-2 goes on the DVD as it is, and a DVD chapter starts only at a closed GOP.
        .args([
            "-g",
            "15",
            "-bf",
            "0",
            "-flags",
            "+cgop",
            "-sc_threshold",
            "1000000000",
        ])
        .args(["-pix_fmt", "yuv420p", "-f", "mpeg2video"])
        .arg(out)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ffmpeg could not make {}", out.display());
}

#[test]
fn a_blu_rays_menu_becomes_a_dvd_menu() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("disccraft-bluray-menu-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (main_video, extra_video) = (dir.join("main.m2v"), dir.join("extra.m2v"));
    video(&ffmpeg, &main_video, 6);
    video(&ffmpeg, &extra_video, 3);

    // The Blu-ray: a menu of three buttons (main title, its chapter 2 at 3 s, the extra title).
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.widescreen = true;
    let main_asset = p.add_asset(&main_video).unwrap();
    let main = p
        .add_timeline(main_asset, Some("Main".to_string()))
        .unwrap();
    p.add_chapter(main, Tick(90 * FRAME), None).unwrap();
    let extra_asset = p.add_asset(&extra_video).unwrap();
    let extra = p
        .add_timeline(extra_asset, Some("Extra".to_string()))
        .unwrap();
    let menu = p.add_menu(Some("Top".to_string())).unwrap();
    p.menu_mut(menu).unwrap().buttons.clear();
    let rects = [
        Rect {
            x: 100,
            y: 300,
            w: 160,
            h: 40,
        },
        Rect {
            x: 300,
            y: 300,
            w: 160,
            h: 40,
        },
        Rect {
            x: 500,
            y: 300,
            w: 160,
            h: 40,
        },
    ];
    for (label, rect, link) in [
        (
            "Play",
            rects[0],
            Link::Timeline {
                id: main,
                chapter: 0,
            },
        ),
        (
            "Scene 2",
            rects[1],
            Link::Timeline {
                id: main,
                chapter: 1,
            },
        ),
        (
            "Extra",
            rects[2],
            Link::Timeline {
                id: extra,
                chapter: 0,
            },
        ),
    ] {
        p.add_button(menu, label.to_string(), Some(rect), link)
            .unwrap();
    }
    for timeline in [main, extra] {
        p.timeline_mut(timeline).unwrap().end_action = Link::Menu {
            id: menu,
            button: None,
        };
    }
    p.settings.first_play = Link::Menu {
        id: menu,
        button: None,
    };
    let disc = dir.join("disc");
    build(
        &p,
        &BuildOptions {
            output: OutputKind::Folder,
            path: disc.clone(),
            cache: Some(dir.join("cache")),
            audio_language: None,
        },
    )
    .unwrap();

    // The DVD project: the main title imported first, then the disc's menu.
    let mut session = Session::new();
    session
        .execute("disc.setAspect", &json!({"aspect": "16:9"}))
        .unwrap();
    let path = disc.to_string_lossy().into_owned();
    let imported = session
        .execute("timeline.importBluray", &json!({ "path": path }))
        .unwrap();
    let main_timeline = imported["timeline"].as_u64().unwrap();
    let made = session
        .execute("menu.importBluray", &json!({ "path": path }))
        .unwrap();
    assert_eq!(made["buttons"], 3, "{made}");
    let timelines: Vec<u64> = made["timelines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_u64().unwrap())
        .collect();
    assert_eq!(timelines.len(), 2, "{made}");
    assert!(
        timelines.contains(&main_timeline),
        "the main title is used again: {made}"
    );
    let extra_timeline = *timelines.iter().find(|id| **id != main_timeline).unwrap();

    let menu_id = disccraft_project::MenuId(made["menu"].as_u64().unwrap());
    let dvd_menu = session.project.menu(menu_id).unwrap().clone();
    let links: Vec<Link> = dvd_menu
        .buttons
        .iter()
        .map(|button| button.link.clone())
        .collect();
    assert_eq!(
        links,
        [
            Link::Timeline {
                id: TimelineId(main_timeline),
                chapter: 0
            },
            Link::Timeline {
                id: TimelineId(main_timeline),
                chapter: 1
            },
            Link::Timeline {
                id: TimelineId(extra_timeline),
                chapter: 0
            },
        ]
    );
    // The same places (an SD Blu-ray's menu is the DVD's size), each highlighted by a shape of its own.
    for (button, rect) in dvd_menu.buttons.iter().zip(rects) {
        assert!(
            button.rect.x.abs_diff(rect.x) <= 2 && button.rect.y.abs_diff(rect.y) <= 2,
            "{:?} for {rect:?}",
            button.rect
        );
        assert!(button.subpicture.is_some(), "{button:?}");
    }
    assert!(matches!(dvd_menu.background, MenuBackground::Video { .. }));
    // The disc starts with it, and the imported titles go back to it.
    let to_menu = Link::Menu {
        id: menu_id,
        button: None,
    };
    assert_eq!(session.project.settings.first_play, to_menu);
    assert_eq!(
        session
            .project
            .timeline(TimelineId(extra_timeline))
            .unwrap()
            .end_action,
        to_menu
    );
    let check = session.execute("project.check", &json!({})).unwrap();
    assert_eq!(check["errors"], 0, "{check}");

    // A folder that is not a Blu-ray: an error.
    assert!(
        session
            .execute(
                "menu.importBluray",
                &json!({ "path": dir.join("cache").to_string_lossy() })
            )
            .is_err()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
