use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use disccraft_dvd::build_video_ts;
use disccraft_project::{AudioTrack, Link, Project, Tick};
use disccraft_vm::{Key, Location, Vm, explore, load_video_ts};

const TICKS_PER_SECOND: i64 = 254_016_000_000;
static FIXTURE_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn builds_menu_disc() -> Result<(), Box<dyn Error>> {
    let ffmpeg = ffmpeg_path();
    let ffprobe = ffprobe_path();

    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;

    let fixtures = fixtures_dir();
    fs::create_dir_all(&fixtures)?;

    let m2v = fixtures.join("ntsc.m2v");
    let ac3 = fixtures.join("ntsc.ac3");

    // Fixtures generated on an earlier run are enough; ffmpeg is only needed to make them.
    if !m2v.is_file() || !ac3.is_file() {
        if !tool_available(&ffmpeg) {
            println!("skipped: ffmpeg not found and no fixtures");
            return Ok(());
        }
        if !m2v.is_file() {
            create_m2v(&ffmpeg, &m2v)?;
        }
        if !ac3.is_file() {
            create_ac3(&ffmpeg, &ac3)?;
        }
    }
    let ffprobe = tool_available(&ffprobe).then_some(ffprobe);

    let project = build_project(&m2v, &ac3)?;
    let dir = temp_dir("menu_disc");
    let _ = fs::remove_dir_all(&dir);

    let result = run_menu_test(&project, &dir, ffprobe);
    let _ = fs::remove_dir_all(&dir);
    result
}

fn run_menu_test(
    project: &Project,
    dir: &Path,
    ffprobe: Option<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    build_video_ts(project, dir)?;
    let video_ts = dir.join("VIDEO_TS");
    for name in [
        "VIDEO_TS.IFO",
        "VIDEO_TS.BUP",
        "VIDEO_TS.VOB",
        "VTS_01_0.IFO",
        "VTS_01_1.VOB",
    ] {
        check_file(&video_ts.join(name))?;
    }
    // The external decoder check runs when ffprobe is installed.
    if let Some(ffprobe) = ffprobe {
        let codecs = probe_codecs(&ffprobe, &video_ts.join("VIDEO_TS.VOB"))?;
        expect_contains(&codecs, "mpeg2video")?;
        expect_contains(&codecs, "dvd_subtitle")?;
    }

    let model = load_video_ts(&video_ts)?;
    // The project preview's model (made without building) is the disc's: the same commands, buttons, routes and
    // titles (the chapter playlist and the root menu remote included).
    if disccraft_dvd::preview_model(project)? != model {
        return Err(format!(
            "preview model differs from the disc:
{:?}
{model:?}",
            disccraft_dvd::preview_model(project)?
        )
        .into());
    }
    let mut vm = Vm::start(&model)?;
    expect_location(vm.location(), &Location::Menu { pgcn: 1, button: 1 })?;
    // Each button plays its chapter; the end action returns to the menu.
    for chapter in 1..=3u16 {
        for _ in 1..chapter {
            vm.press(Key::Down)?;
        }
        let at = *vm.press(Key::Enter)?;
        expect_location(&at, &Location::Title { title: 1, chapter })?;
        let back = *vm.finish_title()?;
        let Location::Menu { pgcn: 1, .. } = back else {
            return Err(format!("expected menu 1 after chapter {chapter}, got {back:?}").into());
        };
        // Reset the selection to button 1 for the next round.
        while let Location::Menu { button, .. } = *vm.location() {
            if button == 1 {
                break;
            }
            vm.press(Key::Up)?;
        }
    }

    // MENU during the title runs the title set's root menu, which jumps back to the VMGM menu.
    if model
        .titles
        .first()
        .is_none_or(|title| title.menu_remote.is_empty())
    {
        return Err("the title set has no root menu".into());
    }
    let at = *vm.press(Key::Enter)?;
    expect_location(
        &at,
        &Location::Title {
            title: 1,
            chapter: 1,
        },
    )?;
    let back = *vm.menu_key()?;
    let Location::Menu { pgcn: 1, .. } = back else {
        return Err(format!("expected menu 1 after MENU, got {back:?}").into());
    };

    // The menu cell holds for 30 seconds, then the post command runs the end action.
    let vmgi = disccraft_ifo::read_vmgi(&fs::read(video_ts.join("VIDEO_TS.IFO"))?)?;
    let still = vmgi
        .menus
        .as_ref()
        .and_then(|menus| menus.pgcs.first())
        .and_then(|menu| menu.pgc.cells.first())
        .map(|cell| cell.still_time);
    if still != Some(30) {
        return Err(format!("menu still time: {still:?}").into());
    }
    let mut vm = Vm::start(&model)?;
    let ran_out = *vm.time_out_menu()?;
    expect_location(
        &ran_out,
        &Location::Title {
            title: 1,
            chapter: 1,
        },
    )?;

    // The chapter playlist (chapters 3, 1 of the main title) is title 2 in the same title set: the fourth
    // button jumps to it, its PGC lists the master's cells in that order, and its end action returns to the
    // menu.
    if vmgi.titles.len() != 2 {
        return Err(format!("expected 2 VMG titles, got {}", vmgi.titles.len()).into());
    }
    let second = vmgi.titles.get(1).ok_or("missing title 2")?;
    if second.vts != 1 || second.vts_ttn != 2 || second.chapters != 2 {
        return Err(format!("title 2 entry: {second:?}").into());
    }
    let vtsi = disccraft_ifo::read_vtsi(&fs::read(video_ts.join("VTS_01_0.IFO"))?)?;
    let playlist_pgc = vtsi.title_pgc(2).ok_or("missing playlist PGC")?;
    let order: Vec<u8> = playlist_pgc.cells.iter().map(|cell| cell.cell_id).collect();
    if order != vec![3, 1] {
        return Err(format!("playlist cell order: {order:?}").into());
    }
    if vtsi.tmaps.len() != 2 || vtsi.address_cells().len() != 3 {
        return Err("playlist TMAP or cell address table is off".into());
    }
    let mut vm = Vm::start(&model)?;
    for _ in 0..3 {
        let _ = vm.press(Key::Down)?;
    }
    let at = *vm.press(Key::Enter)?;
    expect_location(
        &at,
        &Location::Title {
            title: 2,
            chapter: 1,
        },
    )?;
    let after = *vm.finish_title()?;
    let Location::Menu { pgcn: 1, .. } = after else {
        return Err(format!("expected the menu after the chapter playlist, got {after:?}").into());
    };

    let report = explore(&model);
    let expected: std::collections::BTreeSet<(u8, u16)> =
        [(1, 1), (1, 2), (1, 3), (2, 1)].into_iter().collect();
    if report.chapters_reached != expected {
        return Err(format!("chapters reached: {:?}", report.chapters_reached).into());
    }
    if !report.dead_ends.is_empty() || !report.loops.is_empty() {
        return Err(format!("dead ends {:?}, loops {:?}", report.dead_ends, report.loops).into());
    }
    Ok(())
}

fn expect_location(got: &Location, want: &Location) -> Result<(), Box<dyn Error>> {
    if got == want {
        Ok(())
    } else {
        Err(format!("expected {want:?}, got {got:?}").into())
    }
}

fn build_project(video: &Path, audio: &Path) -> Result<Project, Box<dyn Error>> {
    let mut project = Project::default();
    let v = project.add_asset(video)?;
    let a = project.add_asset(audio)?;
    let t = project.add_timeline(v, Some("Main".to_string()))?;
    project.add_chapter(t, Tick(5 * TICKS_PER_SECOND), None)?;
    project.add_chapter(t, Tick(10 * TICKS_PER_SECOND), None)?;
    let tl = project.timeline_mut(t).ok_or("timeline")?;
    tl.audio.push(AudioTrack {
        asset: a,
        language: "ja".to_string(),
    });
    tl.end_action = Link::LastMenu;
    let menu = project.add_menu(Some("Main menu".to_string()))?;
    for (chapter, label) in ["Play", "Chapter 2", "Chapter 3"].into_iter().enumerate() {
        let chapter = u32::try_from(chapter)?;
        project.add_button(
            menu,
            label.to_string(),
            None,
            Link::Timeline { id: t, chapter },
        )?;
    }
    let highlights = project.add_chapter_playlist(t, Some("Highlights".to_string()))?;
    project.add_chapter_playlist_entry(highlights, 2, None)?;
    project.add_chapter_playlist_entry(highlights, 0, None)?;
    project
        .chapter_playlist_mut(highlights)
        .ok_or("chapter playlist")?
        .end_action = Link::LastMenu;
    project.add_button(
        menu,
        "Highlights".to_string(),
        None,
        Link::ChapterPlaylist { id: highlights },
    )?;
    project.settings.first_play = Link::Menu {
        id: menu,
        button: None,
    };
    // After 30 seconds without a choice the menu plays the main title.
    let main_menu = project.menu_mut(menu).ok_or("menu")?;
    main_menu.end_action = Link::Timeline { id: t, chapter: 0 };
    main_menu.duration_seconds = Some(30);
    // The remote's MENU key during the title returns to the menu.
    project.timeline_mut(t).ok_or("timeline")?.menu_remote = Link::Menu {
        id: menu,
        button: None,
    };
    Ok(project)
}

fn probe_codecs(ffprobe: &Path, vob: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "csv=p=0",
        ])
        .arg(vob)
        .output()?;

    if !output.stderr.is_empty() {
        return Err(format!(
            "ffprobe stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    if !output.status.success() {
        return Err("ffprobe failed".into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn create_m2v(ffmpeg: &Path, path: &Path) -> Result<(), Box<dyn Error>> {
    run_success(
        ffmpeg,
        &[
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=720x480:rate=30000/1001",
            "-t",
            "20",
            "-vf",
            "setfield=tff",
            "-c:v",
            "mpeg2video",
            "-pix_fmt",
            "yuv420p",
            "-b:v",
            "6000k",
            "-maxrate",
            "8000k",
            "-bufsize",
            "1835k",
            "-g",
            "15",
            "-bf",
            "2",
            "-flags",
            "+cgop+ilme+ildct",
            "-sc_threshold",
            "1000000000",
            "-aspect",
            "4:3",
            "-f",
            "mpeg2video",
        ],
        path,
    )
}

fn create_ac3(ffmpeg: &Path, path: &Path) -> Result<(), Box<dyn Error>> {
    run_success(
        ffmpeg,
        &[
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:sample_rate=48000",
            "-t",
            "20",
            "-ac",
            "2",
            "-c:a",
            "ac3",
            "-b:a",
            "192k",
        ],
        path,
    )
}

fn run_success(cmd: &Path, args: &[&str], output_path: &Path) -> Result<(), Box<dyn Error>> {
    let output = Command::new(cmd).args(args).arg(output_path).output()?;
    if !output.status.success() {
        return Err(format!(
            "{} failed: {}",
            cmd.display(),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

fn tool_available(cmd: &Path) -> bool {
    Command::new(cmd)
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn ffmpeg_path() -> PathBuf {
    env::var_os("DISCCRAFT_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

fn ffprobe_path() -> PathBuf {
    env::var_os("DISCCRAFT_FFPROBE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffprobe"))
}

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .unwrap_or(manifest)
}

fn fixtures_dir() -> PathBuf {
    workspace_root().join("target").join("fixtures")
}

fn temp_dir(name: &str) -> PathBuf {
    env::temp_dir().join(format!("disccraft-dvd-{}-{}", std::process::id(), name))
}

fn check_file(path: &Path) -> Result<(), Box<dyn Error>> {
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("missing file {}", path.display()).into())
    }
}

fn expect_contains(haystack: &str, needle: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(format!("expected {needle:?} in ffprobe output {haystack:?}").into())
    }
}

/// A small solid PNG (a slide).
fn solid_png(path: &Path, rgb: [u8; 3]) -> Result<(), Box<dyn Error>> {
    let file = std::io::BufWriter::new(fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, 64, 48);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    let pixels: Vec<u8> = rgb.iter().copied().cycle().take(64 * 48 * 3).collect();
    writer.write_image_data(&pixels)?;
    Ok(())
}

/// The project preview's model (made without building) is the built disc's, with a moving menu that loops a
/// counted number of times before its end action, a second menu, a slideshow and a chapter playlist.
#[test]
fn the_preview_model_is_the_discs() -> Result<(), Box<dyn Error>> {
    let fixtures = fixtures_dir();
    let m2v = fixtures.join("ntsc.m2v");
    if !m2v.is_file() {
        println!("skipped: no fixtures");
        return Ok(());
    }
    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;
    let dir = temp_dir("preview_model");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    let wav = dir.join("song.wav");
    fs::write(&wav, silent_wav(48_000))?;
    let red = dir.join("red.png");
    let blue = dir.join("blue.png");
    solid_png(&red, [200, 30, 30])?;
    solid_png(&blue, [30, 30, 200])?;

    let mut project = Project::default();
    let video = project.add_asset(&m2v)?;
    let song = project.add_asset(&wav)?;
    let red = project.add_asset(&red)?;
    let blue = project.add_asset(&blue)?;
    let t = project.add_timeline(video, Some("Main".to_string()))?;
    project.add_chapter(t, Tick(5 * TICKS_PER_SECOND), None)?;
    project.timeline_mut(t).ok_or("timeline")?.end_action = Link::LastMenu;
    let slides = project.add_slideshow(Some("Slides".to_string()))?;
    project.add_slide(slides, red, Tick(TICKS_PER_SECOND))?;
    project.add_slide(slides, blue, Tick(TICKS_PER_SECOND))?;
    let highlights = project.add_chapter_playlist(t, Some("Highlights".to_string()))?;
    project.add_chapter_playlist_entry(highlights, 1, None)?;
    let top = project.add_menu(Some("Top".to_string()))?;
    let second = project.add_menu(Some("Second".to_string()))?;
    for (label, link) in [
        ("Play", Link::Timeline { id: t, chapter: 0 }),
        ("Slides", Link::Slideshow { id: slides }),
        ("Highlights", Link::ChapterPlaylist { id: highlights }),
        (
            "More",
            Link::Menu {
                id: second,
                button: None,
            },
        ),
    ] {
        project.add_button(top, label.to_string(), None, link)?;
    }
    project.add_button(
        second,
        "Back".to_string(),
        None,
        Link::Menu {
            id: top,
            button: Some(2),
        },
    )?;
    {
        let menu = project.menu_mut(top).ok_or("menu")?;
        // A moving menu (menu audio) played twice, then its end action.
        menu.audio = Some(song);
        menu.loops = Some(disccraft_project::MenuLoops::Times(2));
        menu.end_action = Link::Menu {
            id: second,
            button: None,
        };
    }
    project.settings.first_play = Link::Menu {
        id: top,
        button: None,
    };

    let out = dir.join("out");
    let result = (|| -> Result<(), Box<dyn Error>> {
        build_video_ts(&project, &out)?;
        let model = load_video_ts(&out.join("VIDEO_TS"))?;
        let preview = disccraft_dvd::preview_model(&project)?;
        if preview != model {
            return Err(format!(
                "preview model differs from the disc:
{preview:?}
{model:?}"
            )
            .into());
        }
        // Titles: the timeline (2 chapters), the slideshow (a chapter a slide), the chapter playlist (1 entry).
        let chapters: Vec<u16> = model.titles.iter().map(|title| title.chapters).collect();
        if chapters != [2, 2, 1] {
            return Err(format!("unexpected titles: {:?}", model.titles).into());
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

/// A still menu with audio becomes a looping clip with an LPCM stream, and the VMGI declares the menu audio.
#[test]
fn builds_a_still_menu_with_audio() -> Result<(), Box<dyn Error>> {
    let fixtures = fixtures_dir();
    let m2v = fixtures.join("ntsc.m2v");
    if !m2v.is_file() {
        println!("skipped: no fixtures");
        return Ok(());
    }
    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;
    // One second of 48 kHz stereo silence keeps the debug-build encode short (the loop follows the audio).
    let dir = temp_dir("menu_audio");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    let wav = dir.join("song.wav");
    fs::write(&wav, silent_wav(48_000))?;

    let mut project = Project::default();
    let video = project.add_asset(&m2v)?;
    let song = project.add_asset(&wav)?;
    let t = project.add_timeline(video, Some("Main".to_string()))?;
    let menu = project.add_menu(Some("Top".to_string()))?;
    project.add_button(
        menu,
        "Play".to_string(),
        None,
        Link::Timeline { id: t, chapter: 0 },
    )?;
    project.menu_mut(menu).ok_or("menu")?.audio = Some(song);
    project.settings.first_play = Link::Menu {
        id: menu,
        button: None,
    };

    let out = dir.join("out");
    let result = (|| -> Result<(), Box<dyn Error>> {
        build_video_ts(&project, &out)?;
        let video_ts = out.join("VIDEO_TS");
        let vmgi = disccraft_ifo::read_vmgi(&fs::read(video_ts.join("VIDEO_TS.IFO"))?)?;
        let menus = vmgi.menus.ok_or("no menus")?;
        if menus.audio.is_none() {
            return Err("the VMGI declares no menu audio".into());
        }
        let pgc = &menus.pgcs.first().ok_or("no menu PGC")?.pgc;
        if pgc.audio_streams != 1 || pgc.cells.first().map(|cell| cell.still_time) != Some(0) {
            return Err(format!(
                "menu PGC: audio {} still {:?}",
                pgc.audio_streams,
                pgc.cells.first().map(|cell| cell.still_time)
            )
            .into());
        }
        // The menu VOB carries LPCM (private stream 1, sub-stream 0xA0).
        let vob = fs::read(video_ts.join("VIDEO_TS.VOB"))?;
        let lpcm = vob.windows(4).any(|w| w == [0x00, 0x00, 0x01, 0xBD])
            && vob.windows(1).any(|w| w == [0xA0]);
        if !lpcm {
            return Err("no LPCM packets in the menu VOB".into());
        }
        // The VM still shows the menu, and the preview's model is the disc's.
        let model = load_video_ts(&video_ts)?;
        let preview = disccraft_dvd::preview_model(&project)?;
        if preview != model {
            return Err(format!(
                "preview model differs from the disc:
{preview:?}
{model:?}"
            )
            .into());
        }
        let vm = Vm::start(&model)?;
        expect_location(vm.location(), &Location::Menu { pgcn: 1, button: 1 })
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

/// A 16-bit stereo 48 kHz WAV of `frames` silent sample frames.
fn silent_wav(frames: u32) -> Vec<u8> {
    let data = frames * 4;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&48_000u32.to_le_bytes());
    out.extend_from_slice(&192_000u32.to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    out.resize(out.len() + data as usize, 0);
    out
}
