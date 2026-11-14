//! M8.3 check: a Blu-ray project becomes a `BDMV` folder that libbluray (ffmpeg's `bluray:` input) opens, lists with the
//! expected duration and decodes without errors. Skipped when ffmpeg is not available.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_bd::build_bdmv;
use disccraft_bdmv::mobj::NavCommand;
use disccraft_project::{
    AudioTrack, DiscKind, Link, MenuBackground, MenuLoops, Project, SubtitleTrack, Tick, UserOp,
};
use std::path::{Path, PathBuf};
use std::process::Command;

const TICKS_PER_SECOND: i64 = 254_016_000_000;
/// ffmpeg's `bluray:` input ignores playlists shorter than 180 seconds.
const SECONDS: u32 = 190;

fn tool(env: &str, name: &str) -> Option<PathBuf> {
    let path = std::env::var_os(env).map_or_else(|| PathBuf::from(name), PathBuf::from);
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

/// A long, DVD-compliant MPEG-2 + AC-3 pair (passed through without transcoding).
fn long_fixture(ffmpeg: &Path) -> (PathBuf, PathBuf) {
    let video = fixtures().join("bd_long.m2v");
    let audio = fixtures().join("bd_long.ac3");
    if !video.is_file() || !audio.is_file() {
        std::fs::create_dir_all(fixtures()).unwrap();
        let seconds = SECONDS.to_string();
        let ok = Command::new(ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .args(["testsrc=size=720x480:rate=30000/1001", "-t", &seconds])
            .args(["-vf", "setdar=4/3", "-c:v", "mpeg2video", "-b:v", "1500k"])
            .args([
                "-maxrate", "6M", "-bufsize", "1835k", "-g", "15", "-bf", "0",
            ])
            .args(["-pix_fmt", "yuv420p", "-f", "mpeg2video"])
            .arg(&video)
            .status()
            .unwrap()
            .success()
            && Command::new(ffmpeg)
                .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
                .arg(format!(
                    "sine=frequency=440:sample_rate=48000:duration={SECONDS}"
                ))
                .args(["-ac", "2", "-c:a", "ac3", "-b:a", "192k"])
                .arg(&audio)
                .status()
                .unwrap()
                .success();
        assert!(ok, "ffmpeg could not create the fixtures");
    }
    (video, audio)
}

fn project(video: &Path, audio: &Path) -> Project {
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    let v = p.add_asset(video).unwrap();
    let a = p.add_asset(audio).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    p.timeline_mut(t).unwrap().audio.push(AudioTrack {
        asset: a,
        language: "ja".to_string(),
    });
    let srt = fixtures().join("bd_sub.srt");
    std::fs::write(
        &srt,
        "1
00:00:01,000 --> 00:00:04,000
Subtitle test

",
    )
    .unwrap();
    let s = p.add_asset(&srt).unwrap();
    p.timeline_mut(t).unwrap().subtitles.push(SubtitleTrack {
        asset: s,
        language: "en".to_string(),
    });
    p.add_chapter(t, Tick(63 * TICKS_PER_SECOND), None).unwrap();
    p.add_chapter(t, Tick(127 * TICKS_PER_SECOND), None)
        .unwrap();
    // A chapter playlist (chapters 3, 1) becomes a second PlayList over the same clip.
    let highlights = p
        .add_chapter_playlist(t, Some("Highlights".to_string()))
        .unwrap();
    p.add_chapter_playlist_entry(highlights, 2, None).unwrap();
    p.add_chapter_playlist_entry(highlights, 0, None).unwrap();
    // M6.166: shuffled, as Encore's chapter playlist Shuffle.
    p.chapter_playlist_mut(highlights).unwrap().order = disccraft_project::PlaybackOrder::Shuffle;
    // A pop-up menu over the title: the timeline's remote menu.
    let popup = p.add_menu(Some("Popup".to_string())).unwrap();
    p.add_button(
        popup,
        "Chapter 2".to_string(),
        None,
        Link::Timeline { id: t, chapter: 1 },
    )
    .unwrap();
    p.menu_mut(popup).unwrap().popup = true;
    p.timeline_mut(t).unwrap().menu_remote = Link::Menu {
        id: popup,
        button: None,
    };
    p.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    // M6.162: the timeline prohibits the next chapter key and the disc the Top Menu key.
    p.timeline_mut(t).unwrap().prohibited_ops = vec![UserOp::NextChapter];
    p.settings.prohibited_ops = vec![UserOp::TitleMenu];
    p
}

/// M10.9: the title's pop-up menu reads back out of its clip: one pop-up composition, its one button with HDMV
/// commands.
fn popup_reads_back(out: &Path) {
    let clip = std::fs::read(out.join("BDMV/STREAM/00001.m2ts")).unwrap();
    let segments: Vec<u8> = disccraft_m2tsmux::pes_payloads(&clip, disccraft_m2tsmux::IG_PID)
        .unwrap()
        .concat();
    let compositions = disccraft_pgs::read::interactive_compositions(&segments).unwrap();
    let popup = compositions.first().unwrap();
    assert!(popup.popup);
    let page = popup.pages.first().unwrap();
    assert_eq!(page.buttons.len(), 1);
    assert!(!page.buttons.first().unwrap().commands.is_empty());
}

#[test]
fn project_becomes_a_bdmv_folder_that_libbluray_plays() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let (video, audio) = long_fixture(&ffmpeg);
    let out = fixtures().join("bd_out");
    let report = build_bdmv(&project(&video, &audio), &out).unwrap();
    assert_eq!(report.titles.len(), 2);
    assert_eq!(report.titles.first().unwrap().chapters, 3);
    for file in [
        "index.bdmv",
        "MovieObject.bdmv",
        "PLAYLIST/00001.mpls",
        "PLAYLIST/00002.mpls",
        "CLIPINF/00001.clpi",
        "STREAM/00001.m2ts",
        "BACKUP/index.bdmv",
    ] {
        assert!(out.join("BDMV").join(file).is_file(), "missing {file}");
    }

    // The chapter playlist's PlayList: two PlayItems over clip 00001, the last chapter then the first, and a
    // mark at the start of each.
    let highlights = disccraft_bdmv::mpls::read_mpls(
        &std::fs::read(out.join("BDMV/PLAYLIST/00002.mpls")).unwrap(),
    )
    .unwrap();
    assert_eq!(highlights.items.len(), 2);
    assert_eq!(highlights.marks.len(), 2);
    let main = disccraft_bdmv::mpls::read_mpls(
        &std::fs::read(out.join("BDMV/PLAYLIST/00001.mpls")).unwrap(),
    )
    .unwrap();
    let main_item = main.items.first().unwrap();
    // The title's UO mask: menu call (the disc's) and skip to the next point (the timeline's).
    assert_eq!(main.uo_mask, (1 << 63) | (1 << 59));
    // A chapter playlist keeps the disc's.
    assert_eq!(highlights.uo_mask, 1 << 63);
    // It plays its two entries shuffled.
    assert_eq!(
        highlights.playback,
        disccraft_bdmv::mpls::Playback::Shuffle { count: 2 }
    );
    assert_eq!(main.playback, disccraft_bdmv::mpls::Playback::Sequential);
    // The pop-up menu rides in the title clip as an IG stream the playlist lists.
    assert_eq!(main_item.streams.ig.len(), 1);
    popup_reads_back(&out);
    let first = highlights.items.first().unwrap();
    let second = highlights.items.get(1).unwrap();
    assert_eq!(&first.clip, b"00001");
    assert_eq!(first.in_time, main.marks.get(2).unwrap().time);
    assert_eq!(first.out_time, main_item.out_time);
    assert_eq!(second.in_time, main_item.in_time);
    assert_eq!(second.out_time, main.marks.get(1).unwrap().time);
    let playlist_title = report.titles.get(1).unwrap();
    assert_eq!(playlist_title.chapters, 2);
    assert_eq!(playlist_title.playlist, 2);

    // libbluray lists the playlist with the title duration and the two streams.
    let input = format!("bluray:{}", out.display()).replace('\\', "/");
    let probe = Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name:format=duration",
        ])
        .args(["-of", "compact", &input])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    assert!(
        probe.status.success(),
        "{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    assert!(text.contains("codec_name=mpeg2video"), "{text}");
    assert!(text.contains("codec_name=ac3"), "{text}");
    assert!(text.contains("codec_name=hdmv_pgs_subtitle"), "{text}");

    // The whole title (video, audio, subtitles) decodes without errors through libbluray.
    let decode = Command::new(&ffmpeg)
        .args(["-v", "error", "-i", &input])
        // The overlay decodes the presentation graphics stream as well.
        .args(["-filter_complex", "[0:v][0:s:0]overlay", "-map", "0:a"])
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&decode.stderr);
    assert!(decode.status.success(), "{stderr}");
    assert!(stderr.trim().is_empty(), "decode errors: {stderr}");
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

/// M6.153: a motion menu (a video background with audio and a loop point, played twice before its end action)
/// becomes a clip of MPEG-2 video and LPCM audio that ffmpeg decodes cleanly, a playlist that lists the audio and
/// marks the loop point, and a movie object that plays it again from that mark.
#[test]
fn a_motion_menu_becomes_a_clip_that_loops_with_its_sound() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let (video, _) = long_fixture(&ffmpeg);
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    let v = p.add_asset(&video).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    // One second of silence: the menu audio is padded to the two-second background.
    let wav = fixtures().join("bd_menu_song.wav");
    std::fs::create_dir_all(fixtures()).unwrap();
    std::fs::write(&wav, silent_wav(48_000)).unwrap();
    let song = p.add_asset(&wav).unwrap();
    let menu = p.add_menu(Some("Motion".to_string())).unwrap();
    p.add_button(
        menu,
        "Play".to_string(),
        None,
        Link::Timeline { id: t, chapter: 0 },
    )
    .unwrap();
    let m = p.menu_mut(menu).unwrap();
    m.background = MenuBackground::Video {
        asset: v,
        seconds: 2,
    };
    m.audio = Some(song);
    m.loop_point = Some(Tick(TICKS_PER_SECOND));
    m.loops = Some(MenuLoops::Times(2));
    m.end_action = Link::Timeline { id: t, chapter: 0 };
    p.settings.first_play = Link::Menu {
        id: menu,
        button: None,
    };
    let out = fixtures().join("bd_motion_out");
    build_bdmv(&p, &out).unwrap();

    // The menu's playlist: the audio stream, no still, and a mark at the loop point (the GOP start at 1 s).
    let bdmv = out.join("BDMV");
    let playlist =
        disccraft_bdmv::mpls::read_mpls(&std::fs::read(bdmv.join("PLAYLIST/00002.mpls")).unwrap())
            .unwrap();
    let item = playlist.items.first().unwrap();
    assert_eq!(item.streams.audio.len(), 1);
    assert_eq!(item.streams.ig.len(), 1);
    assert_eq!(item.still_mode, 0);
    assert_eq!(playlist.marks.len(), 2);
    let loop_at = playlist.marks.get(1).unwrap().time - item.in_time;
    assert!((22_500..=45_100).contains(&loop_at), "loop mark {loop_at}");
    // Two seconds of pictures.
    let length = item.out_time - item.in_time;
    assert!((88_000..=92_000).contains(&length), "menu length {length}");

    // Its movie object plays it, counts one more play from the loop point's mark, then the title.
    let objects = disccraft_bdmv::mobj::read_movie_objects(
        &std::fs::read(bdmv.join("MovieObject.bdmv")).unwrap(),
    )
    .unwrap();
    let again = NavCommand::play_pl_mark(2, 1);
    let looping = objects
        .iter()
        .find(|object| object.commands.contains(&again))
        .expect("a movie object plays the menu again from its loop point");
    assert!(looping.commands.contains(&NavCommand::sub_reg(1, 1)));
    // It selects the menu's first button itself (the IG names none, so a later play keeps the viewer's button).
    assert!(
        looping
            .commands
            .contains(&NavCommand::set_button_page(0, 0))
    );
    assert!(looping.commands.contains(&NavCommand::play_pl(2)));

    // The clip's video and audio decode without errors.
    let clip = bdmv.join("STREAM/00002.m2ts");
    let probe = Command::new(&ffprobe)
        .args(["-v", "error", "-show_entries", "stream=codec_name"])
        .args(["-of", "compact"])
        .arg(&clip)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    assert!(text.contains("codec_name=mpeg2video"), "{text}");
    assert!(text.contains("codec_name=pcm_bluray"), "{text}");
    let decode = Command::new(&ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&clip)
        // By PID: ffmpeg takes the IG stream (0x1400) for mp3.
        .args([
            "-map",
            "0:i:0x1011",
            "-map",
            "0:i:0x1100",
            "-f",
            "null",
            "-",
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&decode.stderr);
    assert!(decode.status.success(), "{stderr}");
    // Probing the IG stream as mp3 complains; the decoded streams must not.
    let errors: Vec<&str> = stderr
        .lines()
        .filter(|line| {
            !line.contains("mp3") && !line.trim_start().starts_with("Last message repeated")
        })
        .collect();
    assert!(errors.is_empty(), "decode errors: {stderr}");
}
