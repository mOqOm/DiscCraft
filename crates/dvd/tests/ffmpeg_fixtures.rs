use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use disccraft_dvd::build_video_ts;
use disccraft_ifo::{read_vmgi, read_vtsi};
use disccraft_project::{AudioTrack, DiscKind, Project, Tick, VideoStandard};

const TICKS_PER_SECOND: i64 = 254_016_000_000;
static FIXTURE_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn builds_ntsc_ac3_dvd() -> Result<(), Box<dyn Error>> {
    let ffmpeg = ffmpeg_path();
    let ffprobe = ffprobe_path();
    if !tool_available(&ffmpeg) || !tool_available(&ffprobe) {
        println!("skipped: ffmpeg not found");
        return Ok(());
    }

    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;

    let fixtures = fixtures_dir();
    fs::create_dir_all(&fixtures)?;

    let m2v = fixtures.join("ntsc.m2v");
    let ac3 = fixtures.join("ntsc.ac3");

    if !m2v.is_file() {
        create_m2v(&ffmpeg, &m2v)?;
    }
    if !ac3.is_file() {
        create_ac3(&ffmpeg, &ac3)?;
    }

    let project = build_project(&m2v, &ac3)?;
    let dir = temp_dir("ntsc_ac3");
    let _ = fs::remove_dir_all(&dir);

    let result = run_dvd_test(&project, &dir, &ffprobe, &["mpeg2video", "ac3"]);
    let _ = fs::remove_dir_all(&dir);
    result
}

/// A DTS track (`.dts`) goes on the disc as it is: private stream 1, substream 0x88, coding mode 6 in the title
/// set's audio attributes, and ffmpeg decodes the whole track without an error.
#[test]
fn builds_ntsc_dts_dvd() -> Result<(), Box<dyn Error>> {
    let ffmpeg = ffmpeg_path();
    let ffprobe = ffprobe_path();
    if !tool_available(&ffmpeg) || !tool_available(&ffprobe) {
        println!("skipped: ffmpeg not found");
        return Ok(());
    }

    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;

    let fixtures = fixtures_dir();
    fs::create_dir_all(&fixtures)?;
    let m2v = fixtures.join("ntsc.m2v");
    let dts = fixtures.join("ntsc51.dts");
    if !m2v.is_file() {
        create_m2v(&ffmpeg, &m2v)?;
    }
    if !dts.is_file() {
        // 5.1 at the DVD's full DTS rate (1 509.75 kbit/s: 2 013 bytes a frame).
        run_success(
            &ffmpeg,
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
                "6",
                "-c:a",
                "dca",
                "-strict",
                "-2",
                "-b:a",
                "1509750",
                "-f",
                "dts",
            ],
            &dts,
        )?;
    }

    let project = build_project(&m2v, &dts)?;
    let dir = temp_dir("ntsc_dts");
    let _ = fs::remove_dir_all(&dir);
    let result = (|| -> Result<(), Box<dyn Error>> {
        run_dvd_test(&project, &dir, &ffprobe, &["mpeg2video", "dts"])?;
        let video_ts = dir.join("VIDEO_TS");
        // Audio attributes of the title set (0x200: the number of streams, then 8 bytes a stream): coding mode 6,
        // language present, 6 channels, "ja".
        let ifo = fs::read(video_ts.join("VTS_01_0.IFO"))?;
        let attributes = ifo.get(0x202..0x208).ok_or("short IFO")?;
        if attributes != [0x00, 0x01, 0xC4, 0x05, b'j', b'a'] {
            return Err(format!("audio attributes {attributes:02x?}").into());
        }
        let vob = video_ts.join("VTS_01_1.VOB");
        let probe = Command::new(&ffprobe)
            .args([
                "-v",
                "error",
                "-select_streams",
                "a",
                "-show_entries",
                "stream=id,codec_name,sample_rate,channels",
                "-of",
                "csv=p=0",
            ])
            .arg(&vob)
            .output()?;
        let streams = String::from_utf8_lossy(&probe.stdout);
        for wanted in ["dts", "48000", "0x88"] {
            expect_contains(&streams, wanted)?;
        }
        // The whole track decodes: ffmpeg stops at the first error with -xerror.
        let decode = Command::new(&ffmpeg)
            .args(["-v", "error", "-xerror", "-i"])
            .arg(&vob)
            .args(["-map", "0:a", "-f", "null", "-"])
            .output()?;
        if !decode.status.success() || !decode.stderr.is_empty() {
            return Err(format!("DTS decode: {}", String::from_utf8_lossy(&decode.stderr)).into());
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

#[test]
fn builds_ntsc_lpcm_dvd() -> Result<(), Box<dyn Error>> {
    let ffmpeg = ffmpeg_path();
    let ffprobe = ffprobe_path();
    if !tool_available(&ffmpeg) || !tool_available(&ffprobe) {
        println!("skipped: ffmpeg not found");
        return Ok(());
    }

    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;

    let fixtures = fixtures_dir();
    fs::create_dir_all(&fixtures)?;

    let m2v = fixtures.join("ntsc.m2v");
    let wav = fixtures.join("ntsc.wav");

    if !m2v.is_file() {
        create_m2v(&ffmpeg, &m2v)?;
    }
    if !wav.is_file() {
        create_wav(&ffmpeg, &wav)?;
    }

    let project = build_project(&m2v, &wav)?;
    let dir = temp_dir("ntsc_lpcm");
    let _ = fs::remove_dir_all(&dir);

    let result = run_dvd_test(&project, &dir, &ffprobe, &["mpeg2video", "pcm_dvd"]);
    let _ = fs::remove_dir_all(&dir);
    result
}

/// A video that already fits a DVD and comes packed with its sound (an MPEG-2 program stream, as DVD encoders and
/// a `.vob` give it) is repackaged, not encoded again: the cache holds its video and AC-3 taken out, no transcode,
/// and the disc's pictures are the source's own (the same picture types in the same order).
#[test]
fn a_dvd_compliant_program_stream_is_repackaged() -> Result<(), Box<dyn Error>> {
    let ffmpeg = ffmpeg_path();
    let ffprobe = ffprobe_path();
    if !tool_available(&ffmpeg) || !tool_available(&ffprobe) {
        println!("skipped: ffmpeg not found");
        return Ok(());
    }
    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;
    let fixtures = fixtures_dir();
    fs::create_dir_all(&fixtures)?;
    let mpg = fixtures.join("dvd_compliant.mpg");
    if !mpg.is_file() {
        // ffmpeg's DVD target: MPEG-2 720x480 29.97 with B pictures and AC-3 48 kHz, in a program stream.
        let made = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .args(["testsrc2=size=720x480:rate=30000/1001", "-f", "lavfi", "-i"])
            .args(["sine=frequency=440:sample_rate=48000", "-t", "4"])
            .args(["-target", "ntsc-dvd", "-aspect", "4:3", "-bf", "2"])
            .arg(&mpg)
            .status()?;
        assert!(made.success(), "ffmpeg could not make the fixture");
    }
    let picture_types = |path: &Path| -> Result<String, Box<dyn Error>> {
        let out = Command::new(&ffprobe)
            .args(["-v", "error", "-select_streams", "v:0"])
            .args(["-show_entries", "frame=pict_type", "-of", "csv=p=0"])
            .arg(path)
            .output()?;
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|line| line.trim().trim_end_matches(','))
            .collect::<Vec<_>>()
            .join(""))
    };
    let source_pictures = picture_types(&mpg)?;
    assert!(source_pictures.contains('B'), "{source_pictures}");

    let build = |audio_track: bool, name: &str| -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        project.settings.kind = DiscKind::Dvd;
        project.settings.standard = VideoStandard::Ntsc;
        let video = project.add_asset(&mpg)?;
        let timeline = project.add_timeline(video, Some("Packed".to_string()))?;
        if audio_track {
            // As a new timeline takes its video's sound: the same file as audio track 1.
            project
                .timeline_mut(timeline)
                .ok_or("no timeline")?
                .audio
                .push(disccraft_project::AudioTrack {
                    more: Vec::new(),
                    asset: video,
                    stream: 0,
                    language: "en".to_string(),
                    start: 0,
                    trim_start: 0,
                    trim_end: None,
                });
        }
        let dir = temp_dir(name);
        let _ = fs::remove_dir_all(&dir);
        let result = (|| -> Result<(), Box<dyn Error>> {
            let _ = build_video_ts(&project, &dir)?;
            // The cache: the video and the sound taken out, nothing transcoded.
            let mut cached: Vec<String> = fs::read_dir(dir.join("disccraft-cache"))?
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            cached.sort();
            assert!(
                cached.iter().any(|name| name.ends_with(".demux-video.m2v"))
                    && cached.iter().any(|name| name.ends_with(".demux-ac3.ac3")),
                "{cached:?}"
            );
            assert!(
                cached
                    .iter()
                    .all(|name| name.contains(".demux-") || !name.ends_with(".m2v")),
                "a transcode ran: {cached:?}"
            );
            assert!(
                !cached.iter().any(|name| name.ends_with(".wav")),
                "the sound was decoded: {cached:?}"
            );
            let vob = dir.join("VIDEO_TS").join("VTS_01_1.VOB");
            let streams = probe_codecs(&ffprobe, &vob)?;
            expect_contains(&streams, "mpeg2video")?;
            expect_contains(&streams, "ac3")?;
            // The source's own pictures (ours would be I and P only).
            assert_eq!(picture_types(&vob)?, source_pictures);
            let decoded = Command::new(&ffmpeg)
                .args(["-v", "error", "-xerror", "-i"])
                .arg(&vob)
                .args(["-f", "null", "-"])
                .output()?;
            assert!(
                decoded.status.success() && decoded.stderr.is_empty(),
                "{}",
                String::from_utf8_lossy(&decoded.stderr)
            );
            Ok(())
        })();
        let _ = fs::remove_dir_all(&dir);
        result
    };
    build(false, "packed_own_sound")?;
    build(true, "packed_audio_track")
}

/// M6.350: an MPEG-1 system stream as a VCD has it (352x240, 1150 kbit/s constant, MPEG audio at 44.1 kHz) goes on a
/// DVD with its MPEG-1 video as it is: the title set says MPEG-1 at 352x240 (its menus MPEG-2), the VOB holds
/// MPEG-1 video whose pictures are the source's, and the 44.1 kHz sound is converted.
#[test]
fn a_vcd_mpeg1_stream_goes_on_as_it_is() -> Result<(), Box<dyn Error>> {
    let ffmpeg = ffmpeg_path();
    let ffprobe = ffprobe_path();
    if !tool_available(&ffmpeg) || !tool_available(&ffprobe) {
        println!("skipped: ffmpeg not found");
        return Ok(());
    }
    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;
    let fixtures = fixtures_dir();
    fs::create_dir_all(&fixtures)?;
    let mpg = fixtures.join("vcd_mpeg1.mpg");
    if !mpg.is_file() {
        let made = Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .args(["testsrc=size=352x240:rate=30000/1001", "-f", "lavfi", "-i"])
            .args(["sine=frequency=440:sample_rate=44100", "-t", "3"])
            .args(["-c:v", "mpeg1video", "-b:v", "1150k", "-maxrate", "1150k"])
            .args(["-minrate", "1150k", "-bufsize", "327680", "-g", "15"])
            .args(["-c:a", "mp2", "-b:a", "224k", "-f", "vcd"])
            .arg(&mpg)
            .status()?;
        assert!(made.success(), "ffmpeg could not make the fixture");
    }
    let frames = |path: &Path| -> Result<Vec<String>, Box<dyn Error>> {
        let out = Command::new(&ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(path)
            .args(["-map", "0:v:0", "-f", "framemd5", "-"])
            .output()?;
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|line| line.starts_with("0,"))
            .filter_map(|line| line.rsplit(',').next().map(|hash| hash.trim().to_string()))
            .collect())
    };

    let mut project = Project::default();
    project.settings.kind = DiscKind::Dvd;
    project.settings.standard = VideoStandard::Ntsc;
    let video = project.add_asset(&mpg)?;
    let _ = project.add_timeline(video, Some("VCD".to_string()))?;
    let dir = temp_dir("vcd_mpeg1");
    let _ = fs::remove_dir_all(&dir);
    let result = (|| -> Result<(), Box<dyn Error>> {
        let _ = build_video_ts(&project, &dir)?;
        let ifo = fs::read(dir.join("VIDEO_TS").join("VTS_01_0.IFO"))?;
        // Menus MPEG-2 (0x43); the title MPEG-1 (coding mode 0), 4:3, 352x240.
        assert_eq!(ifo.get(0x100..0x102), Some(&[0x43, 0x00][..]));
        assert_eq!(ifo.get(0x200..0x202), Some(&[0x03, 0x18][..]));
        let vob = dir.join("VIDEO_TS").join("VTS_01_1.VOB");
        let streams = probe_codecs(&ffprobe, &vob)?;
        expect_contains(&streams, "mpeg1video")?;
        expect_contains(&streams, "ac3")?;
        let (source, disc) = (frames(&mpg)?, frames(&vob)?);
        assert!(source.len() >= 89, "{}", source.len());
        assert_eq!(disc, source);
        Ok(())
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

/// M6.362: an MPEG-1 stream marked variable (bit rate field all ones) goes on a DVD as it is when its measured peak
/// fits (1.856 Mbit/s); a faster one is converted.
#[test]
fn a_variable_mpeg1_stream_is_measured() -> Result<(), Box<dyn Error>> {
    let ffmpeg = ffmpeg_path();
    let ffprobe = ffprobe_path();
    if !tool_available(&ffmpeg) || !tool_available(&ffprobe) {
        println!("skipped: ffmpeg not found");
        return Ok(());
    }
    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;
    let fixtures = fixtures_dir();
    fs::create_dir_all(&fixtures)?;
    let make =
        |path: &Path, source: &str, quality: &str, seconds: &str| -> Result<(), Box<dyn Error>> {
            if path.is_file() {
                return Ok(());
            }
            let made = Command::new(&ffmpeg)
                .args([
                    "-v", "error", "-y", "-f", "lavfi", "-i", source, "-f", "lavfi", "-i",
                ])
                .args(["sine=frequency=440:sample_rate=44100", "-t", seconds])
                .args(["-c:v", "mpeg1video", "-q:v", quality, "-g", "15"])
                .args(["-c:a", "mp2", "-b:a", "224k", "-f", "mpeg"])
                .arg(path)
                .status()?;
            assert!(made.success(), "ffmpeg could not make the fixture");
            Ok(())
        };
    let slow = fixtures.join("vbr_mpeg1.mpg");
    let fast = fixtures.join("vbr_mpeg1_fast.mpg");
    make(&slow, "testsrc=size=352x240:rate=30000/1001", "4", "3")?;
    make(
        &fast,
        "nullsrc=size=352x240:rate=30000/1001,geq=random(1)*255:128:128",
        "2",
        "2",
    )?;
    // Both are marked variable.
    for path in [&slow, &fast] {
        let data = fs::read(path)?;
        let at = data
            .windows(4)
            .position(|window| window == [0, 0, 1, 0xB3])
            .ok_or("no sequence header")?;
        let field = data.get(at + 8..at + 11).ok_or("short header")?;
        let rate =
            (u32::from(field[0]) << 10) | (u32::from(field[1]) << 2) | (u32::from(field[2]) >> 6);
        assert_eq!(rate, 0x3FFFF);
    }

    let mut project = Project::default();
    project.settings.kind = DiscKind::Dvd;
    project.settings.standard = VideoStandard::Ntsc;
    let slow_asset = project.add_asset(&slow)?;
    let fast_asset = project.add_asset(&fast)?;
    let slow_title = project.add_timeline(slow_asset, Some("Slow".to_string()))?;
    let fast_title = project.add_timeline(fast_asset, Some("Fast".to_string()))?;
    // The slow one goes on as it is, at its measured rate; the fast one is converted (no rate of its own).
    let rates = disccraft_dvd::timeline_rates(&project);
    let rate_of = |id: disccraft_project::TimelineId| {
        rates
            .iter()
            .find(|rate| rate.timeline == id.0)
            .map(|rate| rate.video)
    };
    let slow_rate = rate_of(slow_title).ok_or("the slow stream is not as it is")?;
    assert!((100_000..=1_856_000).contains(&slow_rate), "{slow_rate}");
    assert_eq!(rate_of(fast_title), None);

    let _ = project.remove_timeline(fast_title);
    let dir = temp_dir("vbr_mpeg1");
    let _ = fs::remove_dir_all(&dir);
    let result = (|| -> Result<(), Box<dyn Error>> {
        let _ = build_video_ts(&project, &dir)?;
        let ifo = fs::read(dir.join("VIDEO_TS").join("VTS_01_0.IFO"))?;
        assert_eq!(ifo.get(0x200..0x202), Some(&[0x03, 0x18][..]));
        let vob = dir.join("VIDEO_TS").join("VTS_01_1.VOB");
        expect_contains(&probe_codecs(&ffprobe, &vob)?, "mpeg1video")?;
        Ok(())
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

/// A gap between two video clips (Encore: a clip moved along the track) plays black: the title is as long as the
/// clips and the gap, decodes without errors, and a picture from the gap is black.
#[test]
fn a_gap_between_clips_plays_black() -> Result<(), Box<dyn Error>> {
    let ffmpeg = ffmpeg_path();
    let ffprobe = ffprobe_path();
    if !tool_available(&ffmpeg) || !tool_available(&ffprobe) {
        println!("skipped: ffmpeg not found");
        return Ok(());
    }
    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;
    let fixtures = fixtures_dir();
    fs::create_dir_all(&fixtures)?;
    let m2v = fixtures.join("ntsc.m2v");
    if !m2v.is_file() {
        create_m2v(&ffmpeg, &m2v)?;
    }

    let mut project = Project::default();
    project.settings.kind = DiscKind::Dvd;
    project.settings.standard = VideoStandard::Ntsc;
    let video = project.add_asset(&m2v)?;
    let timeline = project.add_timeline(video, Some("Gap".to_string()))?;
    // The first clip is 599 frames of 1001/30000 s; the second starts 45 frames after it ends.
    let frame = 8_475_667_200_i64;
    let join = Tick(599 * frame);
    assert_eq!(project.append_timeline_video(timeline, video, join)?, 1);
    project.move_timeline_video(
        timeline,
        1,
        (45, None),
        (join, Tick(599 * frame)),
        Tick(45 * frame),
    )?;
    // The chapter at the join went with the clip.
    let chapters: Vec<i64> = project
        .timeline(timeline)
        .ok_or("no timeline")?
        .chapters
        .iter()
        .map(|chapter| chapter.time.0 / frame)
        .collect();
    assert_eq!(chapters, [0, 644]);

    let dir = temp_dir("clip_gap");
    let _ = fs::remove_dir_all(&dir);
    let result = (|| -> Result<(), Box<dyn Error>> {
        let report = build_video_ts(&project, &dir)?;
        let title = report.titles.first().ok_or("missing report title")?;
        assert_eq!(title.chapters, 2);
        let vob = dir.join("VIDEO_TS").join("VTS_01_1.VOB");
        let counted = Command::new(&ffprobe)
            .args(["-v", "error", "-select_streams", "v:0", "-count_frames"])
            .args(["-show_entries", "stream=nb_read_frames", "-of", "csv=p=0"])
            .arg(&vob)
            .output()?;
        assert_eq!(
            String::from_utf8_lossy(&counted.stdout)
                .trim()
                .trim_end_matches(','),
            "1243"
        );
        let decoded = Command::new(&ffmpeg)
            .args(["-v", "error", "-xerror", "-i"])
            .arg(&vob)
            .args(["-f", "null", "-"])
            .output()?;
        assert!(
            decoded.status.success() && decoded.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        // Frame 620 is in the gap (599..644): every sample of its picture is dark.
        let picture = Command::new(&ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(&vob)
            .args(["-vf", "select=eq(n\\,620)", "-frames:v", "1"])
            .args(["-f", "rawvideo", "-pix_fmt", "gray", "-"])
            .output()?;
        assert_eq!(picture.stdout.len(), 720 * 480);
        let brightest = picture.stdout.iter().copied().max().unwrap_or(255);
        assert!(
            brightest < 40,
            "the gap's picture is not black: {brightest}"
        );
        Ok(())
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

/// A timeline of two video clips (Encore: a second video dropped on the video track is butted on to the first, with
/// a chapter at the join) is one title that plays both: twice the frames, decodable without errors.
#[test]
fn builds_a_timeline_of_two_clips() -> Result<(), Box<dyn Error>> {
    let ffmpeg = ffmpeg_path();
    let ffprobe = ffprobe_path();
    if !tool_available(&ffmpeg) || !tool_available(&ffprobe) {
        println!("skipped: ffmpeg not found");
        return Ok(());
    }
    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;
    let fixtures = fixtures_dir();
    fs::create_dir_all(&fixtures)?;
    let m2v = fixtures.join("ntsc.m2v");
    if !m2v.is_file() {
        create_m2v(&ffmpeg, &m2v)?;
    }

    let mut project = Project::default();
    project.settings.kind = DiscKind::Dvd;
    project.settings.standard = VideoStandard::Ntsc;
    let video = project.add_asset(&m2v)?;
    let timeline = project.add_timeline(video, Some("Two clips".to_string()))?;
    // The first clip is 599 frames of 1001/30000 s.
    let join = Tick(599 * 8_475_667_200);
    assert_eq!(project.append_timeline_video(timeline, video, join)?, 1);

    let dir = temp_dir("two_clips");
    let _ = fs::remove_dir_all(&dir);
    let result = (|| -> Result<(), Box<dyn Error>> {
        let report = build_video_ts(&project, &dir)?;
        let title = report.titles.first().ok_or("missing report title")?;
        assert_eq!(title.chapters, 2);
        // 1198 frames: 39.97 s.
        let expected = 1198_i64 * 3003;
        assert!((i64::from(title.duration_90k) - expected).abs() < 45_000);

        let vob = dir.join("VIDEO_TS").join("VTS_01_1.VOB");
        let counted = Command::new(&ffprobe)
            .args(["-v", "error", "-select_streams", "v:0", "-count_frames"])
            .args(["-show_entries", "stream=nb_read_frames", "-of", "csv=p=0"])
            .arg(&vob)
            .output()?;
        assert_eq!(
            String::from_utf8_lossy(&counted.stdout)
                .trim()
                .trim_end_matches(','),
            "1198"
        );
        let decoded = Command::new(&ffmpeg)
            .args(["-v", "error", "-xerror", "-i"])
            .arg(&vob)
            .args(["-f", "null", "-"])
            .output()?;
        assert!(
            decoded.status.success() && decoded.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        // Trimmed, as Encore trims: the first clip's out point at frame 546 and the second clip's in point at
        // frame 52 (both GOP starts of the 13-frame GOPs): 546 + 547 frames.
        project.trim_timeline_video(
            timeline,
            0,
            disccraft_project::ClipTrim {
                start: 0,
                end: Some(546),
            },
            None,
        )?;
        project.trim_timeline_video(
            timeline,
            1,
            disccraft_project::ClipTrim {
                start: 52,
                end: None,
            },
            None,
        )?;
        let trimmed_dir = temp_dir("two_clips_trimmed");
        let _ = fs::remove_dir_all(&trimmed_dir);
        let report = build_video_ts(&project, &trimmed_dir)?;
        let title = report.titles.first().ok_or("missing report title")?;
        assert!((i64::from(title.duration_90k) - 1093 * 3003).abs() < 45_000);
        let vob = trimmed_dir.join("VIDEO_TS").join("VTS_01_1.VOB");
        let counted = Command::new(&ffprobe)
            .args(["-v", "error", "-select_streams", "v:0", "-count_frames"])
            .args(["-show_entries", "stream=nb_read_frames", "-of", "csv=p=0"])
            .arg(&vob)
            .output()?;
        assert_eq!(
            String::from_utf8_lossy(&counted.stdout)
                .trim()
                .trim_end_matches(','),
            "1093"
        );
        let decoded = Command::new(&ffmpeg)
            .args(["-v", "error", "-xerror", "-i"])
            .arg(&vob)
            .args(["-f", "null", "-"])
            .output()?;
        let _ = fs::remove_dir_all(&trimmed_dir);
        assert!(
            decoded.status.success() && decoded.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        Ok(())
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

/// M13.17: a chapter with a parental alternative makes its title a parental block. The timeline of two clips (two
/// chapters) whose second chapter is for level 5, with bars in its place below: one PGC for levels 5 to 8 (the
/// two chapters), one for 1 to 4 (the first chapter and the bars, muxed after the title's last picture); the title's
/// mask and the disc's parental table say so, a player at level 4 plays the second PGC, the VOB decodes whole.
#[test]
fn a_chapter_with_a_parental_alternative_makes_a_parental_block() -> Result<(), Box<dyn Error>> {
    let ffmpeg = ffmpeg_path();
    if !tool_available(&ffmpeg) {
        println!("skipped: ffmpeg not found");
        return Ok(());
    }
    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;
    let fixtures = fixtures_dir();
    fs::create_dir_all(&fixtures)?;
    let m2v = fixtures.join("ntsc.m2v");
    if !m2v.is_file() {
        create_m2v(&ffmpeg, &m2v)?;
    }
    // 120 frames of bars, coded as the fixture is.
    let bars = fixtures.join("ntsc_bars.m2v");
    if !bars.is_file() {
        run_success(
            &ffmpeg,
            &[
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "smptebars=size=720x480:rate=30000/1001",
                "-t",
                "4",
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
            &bars,
        )?;
    }

    let mut project = Project::default();
    project.settings.kind = DiscKind::Dvd;
    project.settings.standard = VideoStandard::Ntsc;
    let video = project.add_asset(&m2v)?;
    let alternative = project.add_asset(&bars)?;
    let timeline = project.add_timeline(video, Some("Parental".to_string()))?;
    let join = Tick(599 * 8_475_667_200);
    assert_eq!(project.append_timeline_video(timeline, video, join)?, 1);
    project
        .timeline_mut(timeline)
        .ok_or("missing timeline")?
        .parental_chapters = vec![disccraft_project::ParentalChapter {
        chapter: 1,
        level: 5,
        alternative,
    }];

    let dir = temp_dir("parental_block");
    let _ = fs::remove_dir_all(&dir);
    let result = (|| -> Result<(), Box<dyn Error>> {
        let report = build_video_ts(&project, &dir)?;
        let title = report.titles.first().ok_or("missing report title")?;
        assert_eq!(title.chapters, 2);
        assert!((i64::from(title.duration_90k) - 1198 * 3003).abs() < 45_000);

        let video_ts = dir.join("VIDEO_TS");
        let vtsi = read_vtsi(&fs::read(video_ts.join("VTS_01_0.IFO"))?)?;
        assert_eq!(vtsi.pgcs.len(), 2);
        let ids: Vec<u16> = vtsi
            .categories
            .iter()
            .map(|category| category.parental_id)
            .collect();
        assert_eq!(ids, [0x00F0, 0x000F]);
        let (full, cut) = (&vtsi.pgcs[0], &vtsi.pgcs[1]);
        assert_eq!(full.program_entry_cells, [1, 2]);
        assert_eq!(cut.program_entry_cells, [1, 2]);
        let cell_ids = |pgc: &disccraft_ifo::Pgc| {
            pgc.cells
                .iter()
                .map(|cell| cell.cell_id)
                .collect::<Vec<_>>()
        };
        assert_eq!(cell_ids(full), [1, 2]);
        assert_eq!(cell_ids(cut), [1, 3]);
        // The bars follow the title's last picture in the VOB.
        assert!(cut.cells[1].first_sector > full.cells[1].last_sector);

        let vmgi = read_vmgi(&fs::read(video_ts.join("VIDEO_TS.IFO"))?)?;
        assert_eq!(vmgi.titles[0].parental_mask, 0x00FF);
        let table = vmgi.parental.as_ref().ok_or("no parental table")?;
        assert_eq!(table.countries.len(), 249);

        let model = disccraft_vm::load_video_ts(&video_ts)?;
        let japan = u16::from_be_bytes(*b"JP");
        assert_eq!(model.title_variant(1, 8, japan), Some(0));
        assert_eq!(model.title_variant(1, 5, japan), Some(0));
        assert_eq!(model.title_variant(1, 4, japan), Some(1));
        assert_eq!(model.title_variant(1, 15, japan), Some(0));
        // M13.19: the Project Preview's model (no build) chooses the same version at every level.
        let preview = disccraft_dvd::preview_model(&project)?;
        for level in [1, 2, 4, 5, 6, 8, 15] {
            assert_eq!(
                preview.title_variant(1, level, japan),
                model.title_variant(1, level, japan),
                "level {level}"
            );
        }

        let vob = video_ts.join("VTS_01_1.VOB");
        let decoded = Command::new(&ffmpeg)
            .args(["-v", "error", "-xerror", "-i"])
            .arg(&vob)
            .args(["-f", "null", "-"])
            .output()?;
        assert!(
            decoded.status.success() && decoded.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        Ok(())
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

/// A clip that needs a transcode (an MP4) butted on to an MPEG-2 clip: it is transcoded on its own and joined, so
/// the title plays both.
#[test]
fn joins_a_clip_that_needs_a_transcode() -> Result<(), Box<dyn Error>> {
    let ffmpeg = ffmpeg_path();
    let ffprobe = ffprobe_path();
    let fixtures = fixtures_dir();
    let (m2v, mp4) = (fixtures.join("ntsc.m2v"), fixtures.join("h264_aac.mp4"));
    if !tool_available(&ffmpeg) || !tool_available(&ffprobe) || !m2v.is_file() || !mp4.is_file() {
        println!("skipped: ffmpeg or fixtures not found");
        return Ok(());
    }
    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;

    let mut project = Project::default();
    project.settings.kind = DiscKind::Dvd;
    project.settings.standard = VideoStandard::Ntsc;
    let first = project.add_asset(&m2v)?;
    let second = project.add_asset(&mp4)?;
    let timeline = project.add_timeline(first, Some("Mixed".to_string()))?;
    let _ = project.append_timeline_video(timeline, second, Tick(599 * 8_475_667_200))?;

    let dir = temp_dir("mixed_clips");
    let _ = fs::remove_dir_all(&dir);
    let result = (|| -> Result<(), Box<dyn Error>> {
        let report = build_video_ts(&project, &dir)?;
        let title = report.titles.first().ok_or("missing report title")?;
        assert_eq!(title.chapters, 2);
        // 599 frames, then six seconds of the MP4 at the disc's 29.97 frames a second (about 180 frames).
        let frames = i64::from(title.duration_90k) / 3003;
        assert!((770..=790).contains(&frames), "{frames} frames");

        let vob = dir.join("VIDEO_TS").join("VTS_01_1.VOB");
        let decoded = Command::new(&ffmpeg)
            .args(["-v", "error", "-xerror", "-i"])
            .arg(&vob)
            .args(["-f", "null", "-"])
            .output()?;
        assert!(
            decoded.status.success() && decoded.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        Ok(())
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

fn run_dvd_test(
    project: &Project,
    dir: &Path,
    ffprobe: &Path,
    expected_codecs: &[&str],
) -> Result<(), Box<dyn Error>> {
    let report = build_video_ts(project, dir)?;
    let video_ts = dir.join("VIDEO_TS");

    for name in [
        "VIDEO_TS.IFO",
        "VIDEO_TS.BUP",
        "VTS_01_0.IFO",
        "VTS_01_0.BUP",
        "VTS_01_1.VOB",
    ] {
        check_file(&video_ts.join(name))?;
    }

    let vmgi_data = fs::read(video_ts.join("VIDEO_TS.IFO"))?;
    let vmgi = read_vmgi(&vmgi_data)?;
    if vmgi.titles.len() != 1 {
        return Err(format!("expected one VMG title, got {}", vmgi.titles.len()).into());
    }
    let title = vmgi.titles.first().ok_or("missing VMG title")?;
    if title.chapters != 3 {
        return Err(format!("expected 3 chapters, got {}", title.chapters).into());
    }

    let vtsi_data = fs::read(video_ts.join("VTS_01_0.IFO"))?;
    let vtsi = read_vtsi(&vtsi_data)?;
    let cells = vtsi.pgcs.first().map_or(0, |pgc| pgc.cells.len());
    if cells != 3 {
        return Err(format!("expected 3 cells, got {cells}").into());
    }

    if report.titles.len() != 1 {
        return Err(format!("expected one report title, got {}", report.titles.len()).into());
    }
    let report_title = report.titles.first().ok_or("missing report title")?;
    if report_title.chapters != 3 {
        return Err(format!("expected report chapters 3, got {}", report_title.chapters).into());
    }

    let expected = i64::from(20) * 90_000;
    let got = i64::from(report_title.duration_90k);
    let diff = got
        .checked_sub(expected)
        .and_then(|x| x.checked_abs())
        .ok_or("duration overflow")?;
    if diff > 45_000 {
        return Err(format!("duration {got} is not within 0.5 s of {expected}").into());
    }

    let stdout = probe_codecs(ffprobe, &video_ts.join("VTS_01_1.VOB"))?;
    for codec in expected_codecs {
        expect_contains(&stdout, codec)?;
    }

    Ok(())
}

fn build_project(video: &Path, audio: &Path) -> Result<Project, Box<dyn Error>> {
    let mut project = Project::default();
    project.settings.kind = DiscKind::Dvd;
    project.settings.standard = VideoStandard::Ntsc;

    let video_asset = project.add_asset(video)?;
    let audio_asset = project.add_asset(audio)?;
    let timeline = project.add_timeline(video_asset, Some("NTSC".to_string()))?;

    project.add_chapter(
        timeline,
        Tick(5 * TICKS_PER_SECOND),
        Some("Chapter 2".to_string()),
    )?;
    project.add_chapter(
        timeline,
        Tick(10 * TICKS_PER_SECOND),
        Some("Chapter 3".to_string()),
    )?;

    let timeline = project.timeline_mut(timeline).ok_or("missing timeline")?;
    timeline.audio.push(AudioTrack {
        more: Vec::new(),
        asset: audio_asset,
        stream: 0,
        language: "ja".to_string(),
        start: 0,
        trim_start: 0,
        trim_end: None,
    });

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

fn create_wav(ffmpeg: &Path, path: &Path) -> Result<(), Box<dyn Error>> {
    run_success(
        ffmpeg,
        &[
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "20",
            "-ac",
            "2",
            "-c:a",
            "pcm_s16le",
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

/// M6.376: why a video is converted for a Blu-ray title rather than going on as it is: the first thing that does
/// not fit (H.264 level 5.1, HEVC, another size, interlaced on a progressive format), and nothing for one that fits.
#[test]
fn a_blu_ray_conversion_says_why() -> Result<(), Box<dyn Error>> {
    use disccraft_dvd::{BdMismatch, BdVideoFormat, bd_mismatch};
    use disccraft_project::BdVideo;
    let ffmpeg = ffmpeg_path();
    if !tool_available(&ffmpeg) {
        println!("skipped: ffmpeg not found");
        return Ok(());
    }
    let _guard = FIXTURE_LOCK.lock().map_err(|_| "fixture lock poisoned")?;
    let fixtures = fixtures_dir();
    fs::create_dir_all(&fixtures)?;
    let make = |name: &str, codec: &[&str]| -> Result<PathBuf, Box<dyn Error>> {
        let path = fixtures.join(name);
        if !path.is_file() {
            let made = Command::new(&ffmpeg)
                .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
                .arg("testsrc2=size=1920x1080:rate=24000/1001")
                .args(["-t", "1", "-pix_fmt", "yuv420p"])
                .args(codec)
                .arg(&path)
                .status()?;
            assert!(made.success(), "ffmpeg could not make {name}");
        }
        Ok(path)
    };
    let p23 = BdVideoFormat::of_video(BdVideo::Hd1080p23).ok_or("no format")?;
    let fits = make(
        "why_fits.mkv",
        &["-c:v", "libx264", "-profile:v", "high", "-level", "4.1"],
    )?;
    let level = make(
        "why_level51.mkv",
        &["-c:v", "libx264", "-profile:v", "high", "-level", "5.1"],
    )?;
    let hevc = make(
        "why_hevc.mkv",
        &["-c:v", "libx265", "-x265-params", "log-level=error"],
    )?;
    assert_eq!(bd_mismatch(&fits, p23), None);
    assert_eq!(bd_mismatch(&level, p23), Some(BdMismatch::Level(51)));
    assert_eq!(bd_mismatch(&hevc, p23), Some(BdMismatch::Codec));
    // The same file on a 720p disc: another size. Interlaced H.264 on a progressive disc.
    let p720 = BdVideoFormat::of_video(BdVideo::Hd720p23).ok_or("no format")?;
    assert_eq!(bd_mismatch(&fits, p720), Some(BdMismatch::Size(1920, 1080)));
    let interlaced = fixtures.join("h264_1080i.mkv");
    if interlaced.is_file() {
        assert_eq!(bd_mismatch(&interlaced, p23), Some(BdMismatch::Scan(true)));
    }
    Ok(())
}
