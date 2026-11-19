//! M6.280 check: File ▸ Render makes the motion menus and the slideshows ahead of a build, into the cache the build
//! uses; the build then takes them, and the disc is the same as one built from an empty cache.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_build::{BuildOptions, OutputKind, Render, build, render};
use disccraft_project::{Link, MenuBackground, Project, Tick};
use std::path::{Path, PathBuf};

const TICKS_PER_SECOND: i64 = 254_016_000_000;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures")
}

/// The names of the files in `dir` that start with `prefix` and end with `suffix`.
fn files(dir: &Path, prefix: &str, suffix: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with(prefix) && name.ends_with(suffix))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The VOB files of a built disc, by name.
fn vobs(disc: &Path) -> Vec<(String, Vec<u8>)> {
    let video_ts = disc.join("VIDEO_TS");
    files(&video_ts, "", ".VOB")
        .into_iter()
        .map(|name| {
            let bytes = std::fs::read(video_ts.join(&name)).unwrap();
            (name, bytes)
        })
        .collect()
}

#[test]
fn rendered_menus_and_slideshows_are_taken_by_the_build() {
    let video = fixtures().join("ntsc.m2v");
    let picture = fixtures().join("menu_render.png");
    if !video.is_file() || !picture.is_file() {
        eprintln!("ntsc.m2v / menu_render.png fixtures not found; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("disccraft-render-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // A motion menu over the video's first two seconds, a title, and a slideshow of one picture.
    let mut project = Project::default();
    let v = project.add_asset(&video).unwrap();
    let still = project.add_asset(&picture).unwrap();
    let t = project.add_timeline(v, Some("Main".to_string())).unwrap();
    let menu = project.add_menu(Some("Motion".to_string())).unwrap();
    project
        .add_button(
            menu,
            "Play".to_string(),
            None,
            Link::Timeline { id: t, chapter: 0 },
        )
        .unwrap();
    project.menu_mut(menu).unwrap().background = MenuBackground::Video {
        asset: v,
        seconds: 2,
    };
    let show = project.add_slideshow(Some("Slides".to_string())).unwrap();
    project
        .add_slide(show, still, Tick(TICKS_PER_SECOND * 2))
        .unwrap();
    project.settings.first_play = Link::Menu {
        id: menu,
        button: None,
    };
    let options = |name: &str, cache: &str| BuildOptions {
        output: OutputKind::Folder,
        path: dir.join(name),
        cache: Some(dir.join(cache)),
        audio_language: None,
    };

    // Render the motion menu and the slideshow into the cache.
    let rendered = options("rendered", "cache");
    let cache = dir.join("cache");
    // M6.334: not rendered yet, the preview has no motion menu to play.
    assert_eq!(
        disccraft_build::rendered_motion_menu(&project, &rendered, menu),
        None
    );
    let menus = render(
        &project,
        &rendered,
        Render::MotionMenus(None),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(menus, 1);
    assert_eq!(files(&cache, "motion-", ".ok").len(), 1);
    // Rendered: the preview finds it, an MPEG-2 video in the cache.
    let found = disccraft_build::rendered_motion_menu(&project, &rendered, menu).unwrap();
    assert!(found.starts_with(&cache) && found.is_file());
    assert_eq!(found.extension().and_then(|e| e.to_str()), Some("m2v"));
    let shows = render(
        &project,
        &rendered,
        Render::Slideshows(Some(show)),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(shows, 1);
    let made = std::fs::read_dir(&cache).unwrap().count();
    // Rendering again makes nothing new.
    render(
        &project,
        &rendered,
        Render::MotionMenus(None),
        &mut |_, _| {},
    )
    .unwrap();
    render(
        &project,
        &rendered,
        Render::Slideshows(None),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), made);

    // A build with that cache, and one from an empty cache, write the same disc.
    build(&project, &rendered).unwrap();
    build(&project, &options("fresh", "fresh-cache")).unwrap();
    let (taken, fresh) = (vobs(&dir.join("rendered")), vobs(&dir.join("fresh")));
    assert!(!taken.is_empty());
    assert_eq!(
        taken.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        fresh.iter().map(|(name, _)| name).collect::<Vec<_>>()
    );
    for ((name, a), (_, b)) in taken.iter().zip(&fresh) {
        assert!(a == b, "{name} differs");
    }

    // A change to the menu is another menu: rendering makes a second play.
    project.menu_mut(menu).unwrap().buttons[0].label = "Start".to_string();
    // Until then, the preview has nothing of the menu as it is now to play.
    assert_eq!(
        disccraft_build::rendered_motion_menu(&project, &rendered, menu),
        None
    );
    render(
        &project,
        &rendered,
        Render::MotionMenus(Some(menu)),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(files(&cache, "motion-", ".ok").len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}
