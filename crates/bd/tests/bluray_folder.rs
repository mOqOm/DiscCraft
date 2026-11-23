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
        more: Vec::new(),
        asset: a,
        stream: 0,
        language: "ja".to_string(),
        start: 0,
        trim_start: 0,
        trim_end: None,
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
        group: 0,
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
    assert_eq!(first.out_time, main_item.out_time);
    assert_eq!(second.in_time, main_item.in_time);
    chapter_spans_start_at_gops(&out, &main, &highlights);
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
    chapter_playlist_decodes(&ffmpeg, &input);
}

/// M10.38: each chapter playlist entry starts (and the first one ends) at the GOP its chapter mark is in: an EP map
/// entry within the second before the mark, as Encore's chapters do (the fixture's GOPs do not fall on the marks).
fn chapter_spans_start_at_gops(
    out: &Path,
    main: &disccraft_bdmv::mpls::PlayList,
    highlights: &disccraft_bdmv::mpls::PlayList,
) {
    let clip = disccraft_bdmv::clpi::read_clpi(
        &std::fs::read(out.join("BDMV/CLIPINF/00001.clpi")).unwrap(),
    )
    .unwrap();
    // The EP map keeps a PTS to 512 ticks.
    let gop_start = |time: u32| {
        clip.ep_map
            .iter()
            .any(|entry| entry.pts == (u64::from(time) * 2) & !0x1FF)
    };
    let near_before =
        |time: u32, mark: u32| gop_start(time) && time <= mark && mark - time < 45_000;
    let first = highlights.items.first().unwrap();
    let second = highlights.items.get(1).unwrap();
    assert!(near_before(first.in_time, main.marks.get(2).unwrap().time));
    assert!(near_before(
        second.out_time,
        main.marks.get(1).unwrap().time
    ));
}

/// M10.38: the chapter playlist (chapter 3, then chapter 1 of the same clip) decodes without errors. Before its play
/// items started at GOP starts, ffmpeg met pictures of the GOP before the mark and reported "Invalid frame
/// dimensions 0x0". (Its `bluray:` input also lists a second copy of the video on the null PID when a play item
/// starts within the clip, which this decodes past: the playlist's own video stream is the first.)
fn chapter_playlist_decodes(ffmpeg: &Path, input: &str) {
    let chapters = Command::new(ffmpeg)
        .args(["-v", "error", "-playlist", "2", "-i", input])
        .args(["-map", "0:v:0", "-map", "0:a:0", "-f", "null", "-"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&chapters.stderr);
    assert!(chapters.status.success(), "{stderr}");
    assert!(
        stderr.trim().is_empty(),
        "chapter playlist decode errors: {stderr}"
    );
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
        layers: None,
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

/// M6.219: an HD (H.264) title of two trimmed video clips with a gap between them is one stream of the parts: the
/// frames of each part at the HD rate, black for the gap, each part starting on a key frame, decodable without
/// errors. Needs the `ntsc.m2v` fixture the
/// DVD tests make and ffmpeg / ffprobe; skipped without them.
#[test]
fn an_hd_title_joins_trimmed_clips() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let video = fixtures().join("ntsc.m2v");
    if !video.is_file() {
        eprintln!("ntsc.m2v fixture not found; skipping");
        return;
    }
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_video = disccraft_project::BdVideo::Hd720p23;
    let v = p.add_asset(&video).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    // The first clip is 599 frames of 1001/30000 s.
    let _ = p
        .append_timeline_video(t, v, Tick(599 * 8_475_667_200))
        .unwrap();
    // GOP starts of the fixture (13 pictures a GOP): 104 frames of the first clip, 78 of the second.
    p.timeline_mut(t).unwrap().video_trims = vec![
        disccraft_project::ClipTrim {
            start: 0,
            end: Some(104),
        },
        disccraft_project::ClipTrim {
            start: 52,
            end: Some(130),
        },
    ];
    // The second clip starts 45 frames (1.5 s) after the first ends: 36 black frames at 23.976.
    p.timeline_mut(t).unwrap().video_gaps = vec![0, 45];
    p.settings.first_play = Link::Timeline { id: t, chapter: 0 };

    let out = fixtures().join("bd_hd_join_out");
    let _ = std::fs::remove_dir_all(&out);
    let report = build_bdmv(&p, &out).unwrap();
    assert_eq!(report.titles.len(), 1);
    let stream = out.join("BDMV/STREAM/00001.m2ts");

    // 104 frames of 29.97 are 83 of 23.976, then 36 black frames, then 78 frames as 62: the gap starts at
    // packet 84 and the second clip at packet 120, each with a key frame.
    let probe = Command::new(&ffprobe)
        .args(["-v", "error", "-select_streams", "v:0"])
        .args(["-show_entries", "stream=codec_name,width,height"])
        .args(["-show_entries", "packet=flags", "-of", "csv=p=0"])
        .arg(&stream)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let packets: Vec<&&str> = lines.iter().filter(|l| l.contains('_')).collect();
    assert_eq!(packets.len(), 181, "{text}");
    assert!(
        packets.get(83).is_some_and(|flags| flags.starts_with('K')),
        "{text}"
    );
    assert!(
        packets.get(119).is_some_and(|flags| flags.starts_with('K')),
        "{text}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("h264,1280,720")),
        "{text}"
    );

    let decode = Command::new(&ffmpeg)
        .args(["-v", "error", "-xerror", "-i"])
        .arg(&stream)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decode.status.success() && decode.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decode.stderr)
    );
    let _ = std::fs::remove_dir_all(&out);
}

/// M10.14: a 1080i 59.94 disc (the Blu-ray format for 29.97 frames a second in HD) is MPEG-2 MP@HL everywhere:
/// the title (two trimmed clips with a black gap between them, joined), the menu's still, and the clips'
/// attributes (1080i, 29.97). ffmpeg decodes both clips without an error. Needs the `ntsc.m2v` fixture and
/// ffmpeg; skipped without.
#[test]
fn a_1080i_disc_is_mpeg2_hd() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let video = fixtures().join("ntsc.m2v");
    if !video.is_file() {
        eprintln!("ntsc.m2v fixture not found; skipping");
        return;
    }
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_video = disccraft_project::BdVideo::Hd1080i59;
    let v = p.add_asset(&video).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    let _ = p
        .append_timeline_video(t, v, Tick(599 * 8_475_667_200))
        .unwrap();
    p.timeline_mut(t).unwrap().video_trims = vec![
        disccraft_project::ClipTrim {
            start: 0,
            end: Some(30),
        },
        disccraft_project::ClipTrim {
            start: 52,
            end: Some(82),
        },
    ];
    p.timeline_mut(t).unwrap().video_gaps = vec![0, 15];
    let menu = p.add_menu(Some("Main menu".to_string())).unwrap();
    p.add_button(
        menu,
        "Play".to_string(),
        None,
        Link::Timeline { id: t, chapter: 0 },
    )
    .unwrap();
    p.settings.first_play = Link::Menu {
        id: menu,
        button: None,
    };

    let out = fixtures().join("bd_1080i_out");
    let _ = std::fs::remove_dir_all(&out);
    let report = build_bdmv(&p, &out).unwrap();
    assert_eq!(report.titles.len(), 1);

    let mut clips: Vec<PathBuf> = std::fs::read_dir(out.join("BDMV/STREAM"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    clips.sort();
    assert_eq!(clips.len(), 2, "{clips:?}");
    let mut frames = Vec::new();
    for stream in &clips {
        // The same frame rate as the source: 30 + 15 + 30 frames in the title, one in the menu.
        let probe = Command::new(&ffprobe)
            .args(["-v", "error", "-select_streams", "v:0", "-count_packets"])
            .args(["-show_entries"])
            .args(["stream=codec_name,profile,level,width,height,r_frame_rate,nb_read_packets"])
            .args(["-of", "csv=p=0"])
            .arg(stream)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&probe.stdout);
        let line = text.lines().next().unwrap_or_default().trim().to_string();
        let count = line
            .trim_end_matches(',')
            .rsplit(',')
            .next()
            .unwrap_or_default()
            .to_string();
        // Main profile at High level (level_idc 4).
        assert!(
            line.starts_with("mpeg2video,Main,1920,1080,4,"),
            "{stream:?}: {text}"
        );
        let count = count.parse::<u32>().unwrap_or(0);
        // (ffprobe guesses the rate of the menu's single picture from its fields.)
        assert!(
            count == 1 || line.contains(",30000/1001,"),
            "{stream:?}: {text}"
        );
        frames.push(count);

        let decode = Command::new(&ffmpeg)
            .args(["-v", "error", "-xerror", "-err_detect", "explode", "-i"])
            .arg(stream)
            .args(["-map", "0:v:0", "-f", "null", "-"])
            .output()
            .unwrap();
        // (Probing a menu clip, ffmpeg tries its short graphics stream as MP3 audio and says so.)
        let errors = String::from_utf8_lossy(&decode.stderr);
        let errors: Vec<&str> = errors
            .lines()
            .filter(|line| !line.contains("mp3float") && !line.contains("Last message repeated"))
            .collect();
        assert!(
            decode.status.success() && errors.is_empty(),
            "{stream:?}: {errors:?}"
        );
    }
    frames.sort_unstable();
    assert_eq!(frames, vec![1, 75]);

    // Every clip says MPEG-2, 1080i, 29.97, 16:9.
    for entry in std::fs::read_dir(out.join("BDMV/CLIPINF")).unwrap() {
        let path = entry.unwrap().path();
        let info = disccraft_bdmv::clpi::read_clpi(&std::fs::read(&path).unwrap()).unwrap();
        let text = format!("{info:?}");
        assert!(
            text.contains("Mpeg2Video { video_format: 4, frame_rate: 4, aspect: 3 }"),
            "{path:?}: {text}"
        );
    }
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&out);
    }
}

/// M12.28: a 1080i 59.94 disc whose codec is H.264 is H.264 coded as interlaced frames everywhere: the title (two
/// trimmed clips with a black gap between them, joined) and the menu's still are H.264 of 1920 x 1080, top field
/// first, that ffmpeg decodes without an error; the clips say H.264, 1080i, 29.97, 16:9. Needs the `ntsc.m2v`
/// fixture and ffmpeg; skipped without.
#[test]
fn a_1080i_disc_can_be_interlaced_h264() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let video = fixtures().join("ntsc.m2v");
    if !video.is_file() {
        eprintln!("ntsc.m2v fixture not found; skipping");
        return;
    }
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_video = disccraft_project::BdVideo::Hd1080i59;
    p.settings.bd_codec = disccraft_project::BdCodec::H264;
    let v = p.add_asset(&video).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    let _ = p
        .append_timeline_video(t, v, Tick(599 * 8_475_667_200))
        .unwrap();
    p.timeline_mut(t).unwrap().video_trims = vec![
        disccraft_project::ClipTrim {
            start: 0,
            end: Some(30),
        },
        disccraft_project::ClipTrim {
            start: 52,
            end: Some(82),
        },
    ];
    p.timeline_mut(t).unwrap().video_gaps = vec![0, 15];
    let menu = p.add_menu(Some("Main menu".to_string())).unwrap();
    p.add_button(
        menu,
        "Play".to_string(),
        None,
        Link::Timeline { id: t, chapter: 0 },
    )
    .unwrap();
    p.settings.first_play = Link::Menu {
        id: menu,
        button: None,
    };

    let out = fixtures().join("bd_1080i_h264_out");
    let _ = std::fs::remove_dir_all(&out);
    let report = build_bdmv(&p, &out).unwrap();
    assert_eq!(report.titles.len(), 1);

    let mut clips: Vec<PathBuf> = std::fs::read_dir(out.join("BDMV/STREAM"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    clips.sort();
    assert_eq!(clips.len(), 2, "{clips:?}");
    let mut frames = Vec::new();
    for stream in &clips {
        let probe = Command::new(&ffprobe)
            .args(["-v", "error", "-select_streams", "v:0", "-count_packets"])
            .args(["-show_entries"])
            .args(["stream=codec_name,width,height,field_order,nb_read_packets"])
            .args(["-of", "csv=p=0"])
            .arg(stream)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&probe.stdout);
        let line = text.lines().next().unwrap_or_default().trim().to_string();
        // Interlaced frames, the top field first.
        assert!(line.starts_with("h264,1920,1080,tt,"), "{stream:?}: {text}");
        let count = line
            .trim_end_matches(',')
            .rsplit(',')
            .next()
            .unwrap_or_default()
            .parse::<u32>()
            .unwrap_or(0);
        frames.push(count);

        let decode = Command::new(&ffmpeg)
            .args(["-v", "error", "-xerror", "-err_detect", "explode", "-i"])
            .arg(stream)
            .args(["-map", "0:v:0", "-f", "null", "-"])
            .output()
            .unwrap();
        // (Probing a menu clip, ffmpeg tries its short graphics stream as MP3 audio and says so.)
        let errors = String::from_utf8_lossy(&decode.stderr);
        let errors: Vec<&str> = errors
            .lines()
            .filter(|line| !line.contains("mp3float") && !line.contains("Last message repeated"))
            .collect();
        assert!(
            decode.status.success() && errors.is_empty(),
            "{stream:?}: {errors:?}"
        );
    }
    frames.sort_unstable();
    // 30 + 15 + 30 frames in the title; the menu's still is one picture.
    assert_eq!(frames, vec![1, 75]);

    // Every clip says H.264, 1080i, 29.97, 16:9.
    for entry in std::fs::read_dir(out.join("BDMV/CLIPINF")).unwrap() {
        let path = entry.unwrap().path();
        let info = disccraft_bdmv::clpi::read_clpi(&std::fs::read(&path).unwrap()).unwrap();
        let text = format!("{info:?}");
        assert!(
            text.contains("H264Video { video_format: 4, frame_rate: 4, aspect: 3 }"),
            "{path:?}: {text}"
        );
    }
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&out);
    }
}

/// M6.221: a subtitle track's colour group reaches the Blu-ray's subtitle stream. With group 2 set to red letters,
/// the picture ffmpeg draws with the subtitle over it has red where the default (white) subtitle has none: at the
/// bottom centre the test picture is yellow and blue. Needs the `ntsc.m2v` fixture and ffmpeg; skipped without.
#[test]
fn a_subtitle_takes_its_colour_group_on_blu_ray() {
    let Some(ffmpeg) = tool("DISCCRAFT_FFMPEG", "ffmpeg") else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let video = fixtures().join("ntsc.m2v");
    if !video.is_file() {
        eprintln!("ntsc.m2v fixture not found; skipping");
        return;
    }
    let srt = fixtures().join("bd_colour_sub.srt");
    std::fs::write(&srt, "1\n00:00:01,000 --> 00:00:04,000\nWWWWWWWW\n\n").unwrap();
    let build = |group: u8, name: &str| {
        let mut p = Project::default();
        p.settings.kind = DiscKind::BluRay;
        let v = p.add_asset(&video).unwrap();
        let s = p.add_asset(&srt).unwrap();
        let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
        p.timeline_mut(t).unwrap().subtitles.push(SubtitleTrack {
            asset: s,
            language: "en".to_string(),
            group,
        });
        let mut colors = disccraft_project::SubtitleColorSet::default();
        colors.groups[1].fill.rgb = [255, 0, 0];
        p.set_timeline_subtitle_colors(t, Some(colors)).unwrap();
        p.settings.first_play = Link::Timeline { id: t, chapter: 0 };
        let out = fixtures().join(name);
        let _ = std::fs::remove_dir_all(&out);
        let _ = build_bdmv(&p, &out).unwrap();
        // One frame at two seconds with the subtitle drawn over it, as raw RGB.
        let frame = Command::new(&ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(out.join("BDMV/STREAM/00001.m2ts"))
            .args(["-filter_complex", "[0:v][0:s:0]overlay", "-ss", "2"])
            .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&out);
        assert_eq!(
            frame.stdout.len(),
            720 * 480 * 3,
            "{}",
            String::from_utf8_lossy(&frame.stderr)
        );
        // Red pixels in the bottom centre (x 240..480, y 380..470).
        let mut red = 0_usize;
        for y in 380..470_usize {
            for x in 240..480_usize {
                let i = (y * 720 + x) * 3;
                if let [r, g, b] = frame.stdout[i..i + 3]
                    && r > 180
                    && g < 80
                    && b < 80
                {
                    red += 1;
                }
            }
        }
        red
    };
    let white = build(0, "bd_colour_white_out");
    let red = build(1, "bd_colour_red_out");
    assert!(white < 20, "white subtitle: {white} red pixels");
    assert!(red > 300, "red subtitle: {red} red pixels");
}

/// A slideshow's "Create Subtitle" slides get their subtitle on an HD Blu-ray title too (a presentation graphics
/// stream in the clip), and a show without such slides has none. Needs the slide pictures the GUI demo makes and
/// ffprobe; skipped without them.
#[test]
fn an_hd_slideshow_carries_its_slides_subtitles() {
    let Some(ffprobe) = tool("DISCCRAFT_FFPROBE", "ffprobe") else {
        eprintln!("ffprobe not found; skipping");
        return;
    };
    let (first, second) = (fixtures().join("slide1.png"), fixtures().join("slide2.png"));
    if !first.is_file() || !second.is_file() {
        eprintln!("slide fixtures not found; skipping");
        return;
    }
    let codecs = |subtitle: bool, name: &str| {
        let mut p = Project::default();
        p.settings.kind = DiscKind::BluRay;
        p.settings.bd_video = disccraft_project::BdVideo::Hd720p23;
        let a = p.add_asset(&first).unwrap();
        let b = p.add_asset(&second).unwrap();
        let show = p.add_slideshow(Some("Slides".to_string())).unwrap();
        let _ = p.add_slide(show, a, Tick(TICKS_PER_SECOND)).unwrap();
        let _ = p.add_slide(show, b, Tick(TICKS_PER_SECOND)).unwrap();
        if subtitle {
            p.set_slide_subtitle(show, 1, true).unwrap();
            let mut colors = disccraft_project::SubtitleColorSet::default();
            colors.groups[0].fill.rgb = [255, 200, 0];
            p.set_slideshow_subtitle_colors(show, Some(colors)).unwrap();
        }
        p.settings.first_play = Link::Slideshow { id: show };
        let out = fixtures().join(name);
        let _ = std::fs::remove_dir_all(&out);
        let _ = build_bdmv(&p, &out).unwrap();
        let probe = Command::new(&ffprobe)
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "csv=p=0",
            ])
            .arg(out.join("BDMV/STREAM/00001.m2ts"))
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&out);
        String::from_utf8_lossy(&probe.stdout).into_owned()
    };
    let plain = codecs(false, "bd_hd_slides_plain_out");
    assert!(plain.contains("h264") && !plain.contains("pgs"), "{plain}");
    let titled = codecs(true, "bd_hd_slides_titled_out");
    assert!(
        titled.contains("h264") && titled.contains("hdmv_pgs_subtitle"),
        "{titled}"
    );
}

/// An HD Blu-ray title makes no DVD (MPEG-2) transcode of its video on the way: the cache holds the H.264 encode
/// and a short black stand-in, not an MPEG-2 file of the source's length; the title still gets the source's sound
/// (as AC-3) and its video. Needs the `h264_aac.mp4` fixture the transcode tests make and ffprobe; skipped without.
/// An HD disc's slideshow is encoded from its slides as H.264 only: no MPEG-2 encode of the show is left in the
/// cache, and the clip has the pictures and the show's sound at the show's length.
#[test]
fn an_hd_slideshow_makes_no_dvd_encode() {
    let Some(ffprobe) = tool("DISCCRAFT_FFPROBE", "ffprobe") else {
        eprintln!("ffprobe not found; skipping");
        return;
    };
    let (first, second) = (fixtures().join("slide1.png"), fixtures().join("slide2.png"));
    if !first.is_file() || !second.is_file() {
        eprintln!("slide fixtures not found; skipping");
        return;
    }
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_video = disccraft_project::BdVideo::Hd720p23;
    let a = p.add_asset(&first).unwrap();
    let b = p.add_asset(&second).unwrap();
    let show = p.add_slideshow(Some("Show".to_string())).unwrap();
    let _ = p.add_slide(show, a, Tick(TICKS_PER_SECOND)).unwrap();
    let _ = p.add_slide(show, b, Tick(TICKS_PER_SECOND)).unwrap();
    p.settings.first_play = Link::Slideshow { id: show };

    let out = fixtures().join("bd_hd_show_no_sd_out");
    let _ = std::fs::remove_dir_all(&out);
    let _ = build_bdmv(&p, &out).unwrap();

    let mpeg2: Vec<(String, u64)> = std::fs::read_dir(out.join("disccraft-cache"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "m2v"))
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.metadata().map_or(0, |metadata| metadata.len()),
            )
        })
        .collect();
    assert_eq!(mpeg2.len(), 1, "{mpeg2:?}");
    assert!(
        mpeg2
            .iter()
            .all(|(name, size)| name.starts_with("stand-in") && *size < 200_000),
        "{mpeg2:?}"
    );

    let probe = Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,duration",
            "-of",
            "csv=p=0",
        ])
        .arg(out.join("BDMV/STREAM/00001.m2ts"))
        .output()
        .unwrap();
    let streams = String::from_utf8_lossy(&probe.stdout);
    assert!(
        streams.contains("h264") && streams.contains("ac3"),
        "{streams}"
    );
    assert!(!streams.contains("mpeg2video"), "{streams}");
    // Two slides of one second each: the pictures and the sound are both about two seconds long.
    let lengths: Vec<f64> = streams
        .lines()
        .filter_map(|line| line.split(',').nth(1)?.trim().parse().ok())
        .collect();
    // (ffprobe lists the streams once more under the program.)
    assert!(lengths.len() >= 2, "{streams}");
    assert!(
        lengths.iter().all(|length| (1.9..2.2).contains(length)),
        "{streams}"
    );
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn an_hd_title_makes_no_dvd_transcode() {
    let Some(ffprobe) = tool("DISCCRAFT_FFPROBE", "ffprobe") else {
        eprintln!("ffprobe not found; skipping");
        return;
    };
    let video = fixtures().join("h264_aac.mp4");
    if !video.is_file() {
        eprintln!("h264_aac.mp4 fixture not found; skipping");
        return;
    }
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_video = disccraft_project::BdVideo::Hd720p23;
    let v = p.add_asset(&video).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    p.settings.first_play = Link::Timeline { id: t, chapter: 0 };

    let out = fixtures().join("bd_hd_no_sd_out");
    let _ = std::fs::remove_dir_all(&out);
    let _ = build_bdmv(&p, &out).unwrap();

    // No MPEG-2 video is made of the timeline: the title comes with its sound, chapters and timing only.
    let mpeg2: Vec<(String, u64)> = std::fs::read_dir(out.join("disccraft-cache"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "m2v"))
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.metadata().map_or(0, |metadata| metadata.len()),
            )
        })
        .collect();
    assert!(mpeg2.is_empty(), "{mpeg2:?}");

    let probe = Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "csv=p=0",
        ])
        .arg(out.join("BDMV/STREAM/00001.m2ts"))
        .output()
        .unwrap();
    let codecs = String::from_utf8_lossy(&probe.stdout);
    assert!(
        codecs.contains("h264") && codecs.contains("ac3"),
        "{codecs}"
    );
    assert!(!codecs.contains("mpeg2video"), "{codecs}");
    let _ = std::fs::remove_dir_all(&out);
}

/// The decoded pictures of `file`'s first video stream as one MD5 (ffmpeg's `md5` muxer).
fn video_md5(ffmpeg: &Path, file: &Path) -> String {
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(file)
        .args(["-map", "0:v:0", "-f", "md5", "-"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{file:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// M10.16: interlaced H.264 in a Matroska file (which the media decoder does not read) goes on a 1080i disc as it
/// is. The source has no access unit delimiters and its parameter sets only in the track's header, B pictures (so
/// its decode times are worked out) and times rounded to milliseconds; the clip decodes to the same pictures.
#[test]
fn interlaced_h264_in_an_mkv_goes_on_a_1080i_disc_as_it_is() {
    interlaced_h264_goes_on_a_1080i_disc_as_it_is("mkv", 1920);
}

/// M10.20: the anamorphic 1440 x 1080 of HDV and AVCHD camcorders is a Blu-ray's video too, and goes on the disc
/// as it is.
#[test]
fn anamorphic_1440_wide_h264_goes_on_a_1080i_disc_as_it_is() {
    interlaced_h264_goes_on_a_1080i_disc_as_it_is("mkv", 1440);
}

/// M10.17: the same from an MP4 file, whose samples have their own decode times and whose key frames are its sync
/// samples.
#[test]
fn interlaced_h264_in_an_mp4_goes_on_a_1080i_disc_as_it_is() {
    interlaced_h264_goes_on_a_1080i_disc_as_it_is("mp4", 1920);
}

fn interlaced_h264_goes_on_a_1080i_disc_as_it_is(container: &str, width: u32) {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("ffmpeg / ffprobe not found; skipping");
        return;
    };
    let name = if width == 1920 {
        "h264_1080i".to_owned()
    } else {
        format!("h264_{width}x1080i")
    };
    let video = fixtures().join(format!("{name}.{container}"));
    if !video.is_file() {
        std::fs::create_dir_all(fixtures()).unwrap();
        let made = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!("testsrc2=size={width}x1080:rate=30000/1001"))
            .args(["-f", "lavfi", "-i"])
            .args(["sine=frequency=440:sample_rate=48000", "-t", "3"])
            .args([
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-profile:v",
                "high",
            ])
            .args(["-level", "4.1", "-pix_fmt", "yuv420p", "-b:v", "6M"])
            .args(["-flags", "+ildct+ilme", "-x264-params"])
            .args(["tff=1:bframes=2:keyint=30:aud=0:8x8dct=1:vbv-maxrate=20000:vbv-bufsize=20000"])
            // Anamorphic: shown 16:9.
            .args(["-aspect", "16:9"])
            .args(["-c:a", "aac", "-b:a", "128k"])
            .arg(&video)
            .status();
        if !made.is_ok_and(|status| status.success()) {
            let _ = std::fs::remove_file(&video);
            eprintln!("ffmpeg cannot make interlaced H.264 (no libx264?); skipping");
            return;
        }
    }
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_video = disccraft_project::BdVideo::Hd1080i59;
    let v = p.add_asset(&video).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    p.settings.first_play = Link::Timeline { id: t, chapter: 0 };

    // M12.16: its length, size and rate come from the file's header (the file is not opened as media).
    let probe = disccraft_transcode::probe_video(&video).unwrap();
    assert_eq!((probe.width, probe.height), (width, 1080));
    assert!((probe.fps - 30_000.0 / 1001.0).abs() < 1e-9, "{probe:?}");
    assert!((probe.seconds - 3.0).abs() < 0.1, "{probe:?}");
    assert!(probe.wide && probe.has_audio, "{probe:?}");
    let sound = disccraft_transcode::probe_audio_seconds(&video).unwrap();
    assert!((sound - probe.seconds).abs() < 1e-9);

    // The size estimate counts the file as it is: nothing to encode.
    let estimate = disccraft_dvd::estimate(&p).unwrap();
    assert!(estimate.passthrough_bytes > 0, "{estimate:?}");
    assert!(estimate.transcode_seconds.abs() < 1e-9, "{estimate:?}");

    let out = fixtures().join(format!("bd_{name}_{container}_out"));
    let _ = std::fs::remove_dir_all(&out);
    let report = build_bdmv(&p, &out).unwrap();
    assert_eq!(report.titles.len(), 1);

    let stream = out.join("BDMV/STREAM/00001.m2ts");
    let probe = Command::new(&ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-count_packets"])
        .args(["-show_entries"])
        .args(["stream=codec_name,profile,width,height,field_order,r_frame_rate,nb_read_packets"])
        .args(["-of", "csv=p=0"])
        .arg(&stream)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    let line = text.lines().next().unwrap_or_default().trim();
    assert_eq!(
        line,
        format!("h264,High,{width},1080,tt,30000/1001,90"),
        "{text}"
    );

    // Decodes without an error, to the pictures of the source.
    let decode = Command::new(&ffmpeg)
        .args(["-v", "error", "-xerror", "-err_detect", "explode", "-i"])
        .arg(&stream)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decode.status.success() && decode.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decode.stderr)
    );
    assert_eq!(video_md5(&ffmpeg, &stream), video_md5(&ffmpeg, &video));

    // Decode times rise by a frame and never pass the picture's own time.
    let packets = Command::new(&ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
        .args(["packet=pts,dts", "-of", "csv=p=0"])
        .arg(&stream)
        .output()
        .unwrap();
    let times: Vec<(i64, i64)> = String::from_utf8_lossy(&packets.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.trim().trim_end_matches(',').split(',');
            Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
        })
        .collect();
    assert_eq!(times.len(), 90);
    assert!(times.iter().all(|(pts, dts)| dts <= pts), "{times:?}");
    assert!(
        times.array_windows().all(|[a, b]| b.1 - a.1 == 3003),
        "{times:?}"
    );
    assert!(times.iter().any(|(pts, dts)| pts > dts), "no reordering");

    // The clip says H.264, 1080i, 29.97, 16:9.
    let info = std::fs::read(out.join("BDMV/CLIPINF/00001.clpi")).unwrap();
    let text = format!("{:?}", disccraft_bdmv::clpi::read_clpi(&info).unwrap());
    assert!(
        text.contains("H264Video { video_format: 4, frame_rate: 4, aspect: 3 }"),
        "{text}"
    );
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&out);
    }
}

/// A 24-bit WAV (`WAVE_FORMAT_EXTENSIBLE`, speakers `mask`) of `channels` at `rate` for `seconds`: channel n a
/// tone of 200 + 100 n Hz.
fn tone_wav(rate: u32, channels: u16, mask: u32, seconds: u32) -> Vec<u8> {
    let frames = rate * seconds;
    let mut samples = Vec::with_capacity((frames * u32::from(channels) * 3) as usize);
    for frame in 0..frames {
        for channel in 0..channels {
            let hz = 200.0 + 100.0 * f64::from(channel);
            let phase = std::f64::consts::TAU * hz * f64::from(frame) / f64::from(rate);
            let value = (phase.sin() * 4_000_000.0) as i32;
            samples.extend_from_slice(&value.to_le_bytes()[..3]);
        }
    }
    let block = channels * 3;
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&0xFFFEu16.to_le_bytes());
    fmt.extend_from_slice(&channels.to_le_bytes());
    fmt.extend_from_slice(&rate.to_le_bytes());
    fmt.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
    fmt.extend_from_slice(&block.to_le_bytes());
    fmt.extend_from_slice(&24u16.to_le_bytes());
    fmt.extend_from_slice(&22u16.to_le_bytes());
    fmt.extend_from_slice(&24u16.to_le_bytes());
    fmt.extend_from_slice(&mask.to_le_bytes());
    fmt.extend_from_slice(&[
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b,
        0x71,
    ]);
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(4 + 8 + fmt.len() as u32 + 8 + samples.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
    out.extend_from_slice(&fmt);
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    out.extend_from_slice(&samples);
    out
}

/// M6.347: a 5.1 WAV of 24 bits at 192 kHz goes on a Blu-ray as LPCM as it is (not converted to stereo Dolby
/// Digital): ffmpeg (a black box) reads 5.1 at 192 kHz, and hears each speaker's tone on that speaker.
#[test]
fn a_5_1_wav_at_192_khz_keeps_its_speakers() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("skipping: ffmpeg/ffprobe not found");
        return;
    };
    let video = short_video(&ffmpeg);
    // 5.1 with side surrounds: FL FR FC LFE SL SR.
    let wav = fixtures().join("bd_lpcm51.wav");
    std::fs::write(&wav, tone_wav(192_000, 6, 0x60F, 3)).unwrap();
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    let v = p.add_asset(&video).unwrap();
    let a = p.add_asset(&wav).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    p.timeline_mut(t).unwrap().audio.push(AudioTrack::of(a));
    p.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    let out = fixtures().join("bd_lpcm51_out");
    build_bdmv(&p, &out).unwrap();

    let clip = out.join("BDMV/STREAM/00001.m2ts");
    let probe = Command::new(&ffprobe)
        .args(["-v", "error", "-select_streams", "a"])
        .args([
            "-show_entries",
            "stream=codec_name,sample_rate,channels,channel_layout",
        ])
        .args(["-of", "compact=p=0:nk=1"])
        .arg(&clip)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    // The stream (listed again under its program).
    assert_eq!(
        text.lines().next().map(str::trim),
        Some("pcm_bluray|192000|6|5.1(side)"),
        "{text}"
    );
    let decode = Command::new(&ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&clip)
        .args(["-map", "0:a:0", "-f", "s16le", "-c:a", "pcm_s16le", "-"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&decode.stderr);
    assert!(
        decode.status.success() && stderr.trim().is_empty(),
        "{stderr}"
    );
    let samples: Vec<i16> = decode
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair))
        .collect();
    // A second from half a second in: each channel crosses zero twice a cycle of its tone.
    for channel in 0..6 {
        let tone: Vec<i16> = samples
            .as_chunks::<6>()
            .0
            .iter()
            .skip(96_000)
            .take(192_000)
            .map(|frame| frame[channel])
            .collect();
        let crossings = tone
            .windows(2)
            .filter(|pair| (pair[0] < 0) != (pair[1] < 0))
            .count();
        let hz = 200 + 100 * channel;
        assert!(
            crossings.abs_diff(2 * hz) <= 4,
            "channel {channel}: {crossings} crossings, {hz} Hz wanted"
        );
    }
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&out);
    }
}

/// Three seconds of DVD-compliant MPEG-2 (an SD Blu-ray takes it as it is).
fn short_video(ffmpeg: &Path) -> PathBuf {
    std::fs::create_dir_all(fixtures()).unwrap();
    let video = fixtures().join("bd_lpcm51.m2v");
    if !video.is_file() {
        let ok = Command::new(ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .args(["testsrc=size=720x480:rate=30000/1001", "-t", "3"])
            .args(["-vf", "setdar=4/3", "-c:v", "mpeg2video", "-b:v", "1500k"])
            .args([
                "-maxrate", "6M", "-bufsize", "1835k", "-g", "15", "-bf", "0",
            ])
            .args(["-pix_fmt", "yuv420p", "-f", "mpeg2video"])
            .arg(&video)
            .status()
            .unwrap()
            .success();
        assert!(ok, "ffmpeg could not create the video");
    }
    video
}

/// M6.348: a DTS file goes on a Blu-ray as it is: the clip and the playlist list a DTS stream (0x82, multi-channel,
/// 48 kHz), and ffmpeg (a black box) reads the frames as the file's 5.1 DTS and decodes them without errors.
#[test]
fn a_dts_file_goes_on_a_blu_ray_as_it_is() {
    let (Some(ffmpeg), Some(ffprobe)) = (
        tool("DISCCRAFT_FFMPEG", "ffmpeg"),
        tool("DISCCRAFT_FFPROBE", "ffprobe"),
    ) else {
        eprintln!("skipping: ffmpeg/ffprobe not found");
        return;
    };
    let video = short_video(&ffmpeg);
    let dts = fixtures().join("bd_dts51.dts");
    if !dts.is_file() {
        let ok = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("sine=frequency=440:sample_rate=48000:duration=3")
            .args(["-ac", "6", "-c:a", "dca", "-strict", "-2", "-b:a", "1509k"])
            .arg(&dts)
            .status()
            .unwrap()
            .success();
        assert!(ok, "ffmpeg could not create the DTS file");
    }
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    let v = p.add_asset(&video).unwrap();
    let a = p.add_asset(&dts).unwrap();
    let t = p.add_timeline(v, Some("Main".to_string())).unwrap();
    let mut track = AudioTrack::of(a);
    track.language = "en".to_string();
    p.timeline_mut(t).unwrap().audio.push(track);
    p.settings.first_play = Link::Timeline { id: t, chapter: 0 };
    let out = fixtures().join("bd_dts_out");
    build_bdmv(&p, &out).unwrap();

    let bdmv = out.join("BDMV");
    let info =
        disccraft_bdmv::clpi::read_clpi(&std::fs::read(bdmv.join("CLIPINF/00001.clpi")).unwrap())
            .unwrap();
    let dts_stream = disccraft_bdmv::StreamCoding::Dts {
        audio_format: 6,
        sample_rate: 1,
        language: *b"eng",
    };
    assert!(
        info.streams
            .iter()
            .any(|stream| stream.coding == dts_stream),
        "{info:?}"
    );
    let playlist =
        disccraft_bdmv::mpls::read_mpls(&std::fs::read(bdmv.join("PLAYLIST/00001.mpls")).unwrap())
            .unwrap();
    let audio = &playlist.items.first().unwrap().streams.audio;
    assert_eq!(audio.len(), 1);
    assert_eq!(audio[0].coding, dts_stream);

    let clip = bdmv.join("STREAM/00001.m2ts");
    let probe = Command::new(&ffprobe)
        .args(["-v", "error", "-select_streams", "a"])
        .args(["-show_entries", "stream=codec_name,sample_rate,channels"])
        .args(["-of", "compact=p=0:nk=1"])
        .arg(&clip)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    assert_eq!(
        text.lines().next().map(str::trim),
        Some("dts|48000|6"),
        "{text}"
    );
    let decode = Command::new(&ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&clip)
        .args(["-map", "0:a:0", "-f", "null", "-"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&decode.stderr);
    assert!(
        decode.status.success() && stderr.trim().is_empty(),
        "{stderr}"
    );
    if std::env::var_os("DISCCRAFT_KEEP_OUTPUT").is_none() {
        let _ = std::fs::remove_dir_all(&out);
    }
}

/// M6.368: HD titles are made side by side; two that play the same clip encode it once (the second waits for the
/// first's file), and both titles carry it.
#[test]
fn hd_titles_of_one_clip_encode_it_once() {
    let video = fixtures().join("h264_aac.mp4");
    if !video.is_file() {
        eprintln!("h264_aac.mp4 fixture not found; skipping");
        return;
    }
    let mut p = Project::default();
    p.settings.kind = DiscKind::BluRay;
    p.settings.bd_video = disccraft_project::BdVideo::Hd720p59;
    let v = p.add_asset(&video).unwrap();
    let first = p.add_timeline(v, Some("One".to_string())).unwrap();
    let _ = p.add_timeline(v, Some("Two".to_string())).unwrap();
    p.settings.first_play = Link::Timeline {
        id: first,
        chapter: 0,
    };
    let out = std::env::temp_dir().join(format!("disccraft-hd-once-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let _ = build_bdmv(&p, &out).unwrap();
    let encodes = std::fs::read_dir(out.join("disccraft-cache"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "h264"))
        .count();
    assert_eq!(encodes, 1);
    let size = |name: &str| {
        std::fs::metadata(out.join("BDMV/STREAM").join(name)).map_or(0, |meta| meta.len())
    };
    let (one, two) = (size("00001.m2ts"), size("00002.m2ts"));
    assert!(one > 100_000 && one == two, "{one} {two}");
    let _ = std::fs::remove_dir_all(&out);
}
