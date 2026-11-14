use crate::params::{req_str, req_value};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{Capacity, DiscKind, VideoStandard};
use serde_json::Value;

use super::{always_enabled, empty_object};

/// The longest disc name, in characters.
const MAX_NAME_CHARS: usize = 128;
/// The longest disc description, in characters.
const MAX_DESCRIPTION_CHARS: usize = 4096;

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "disc.setName",
        label: "ディスクの名前を設定",
        menu: &["ディスク"],
        shortcut: None,
        params: r#"{"name": string}"#,
        enabled: always_enabled,
        run: run_set_name,
        journal: true,
    },
    CommandSpec {
        id: "disc.setDescription",
        label: "ディスクの説明を設定",
        menu: &["ディスク"],
        shortcut: None,
        params: r#"{"description": string}"#,
        enabled: always_enabled,
        run: run_set_description,
        journal: true,
    },
    CommandSpec {
        id: "disc.setUserOps",
        label: "ディスクのユーザー操作を設定",
        menu: &["ディスク"],
        shortcut: None,
        params: r#"{"prohibited": ["fastForward", ...]}"#,
        enabled: always_enabled,
        run: run_set_user_ops,
        journal: true,
    },
    CommandSpec {
        id: "disc.setAudio",
        label: "ディスクのオーディオを設定",
        menu: &["ディスク"],
        shortcut: None,
        params: r#"{"track": 1..8 | null}"#,
        enabled: always_enabled,
        run: run_set_audio,
        journal: true,
    },
    CommandSpec {
        id: "disc.setAudioLanguage",
        label: "ディスクのオーディオの言語を設定",
        menu: &["ディスク"],
        shortcut: None,
        params: r#"{"language": "ja" | null}"#,
        enabled: always_enabled,
        run: run_set_audio_language,
        journal: true,
    },
    CommandSpec {
        id: "disc.setLayerBreak",
        label: "レイヤーブレークを設定",
        menu: &["ディスク"],
        shortcut: None,
        params: r#"{"timeline": id, "time": ticks | "timecode": "00;45;12;15"} | {"timeline": null} (自動)"#,
        enabled: always_enabled,
        run: run_set_layer_break,
        journal: true,
    },
    CommandSpec {
        id: "disc.setSubtitle",
        label: "ディスクの字幕を設定",
        menu: &["ディスク"],
        shortcut: None,
        params: r#"{"track": 1..32 | "off" | null}"#,
        enabled: always_enabled,
        run: run_set_subtitle,
        journal: true,
    },
    CommandSpec {
        id: "disc.setKind",
        label: "ディスク種類を設定",
        menu: &["ディスク"],
        shortcut: None,
        params: r#"{"kind": "dvd"|"bluRay"}"#,
        enabled: always_enabled,
        run: run_set_kind,
        journal: true,
    },
    CommandSpec {
        id: "disc.setStandard",
        label: "映像規格を設定",
        menu: &["ディスク"],
        shortcut: None,
        params: r#"{"standard": "ntsc"|"pal"}"#,
        enabled: always_enabled,
        run: run_set_standard,
        journal: true,
    },
    CommandSpec {
        id: "disc.setAspect",
        label: "画面比率を設定",
        menu: &["ディスク"],
        shortcut: None,
        params: r#"{"aspect": "4:3"|"16:9"}"#,
        enabled: always_enabled,
        run: run_set_aspect,
        journal: true,
    },
    CommandSpec {
        id: "disc.setBlurayVideo",
        label: "Blu-ray の映像を設定",
        menu: &["ディスク"],
        shortcut: None,
        params: r#"{"video": "sd"|"1080p23.976"|"1080p24"|"1080p25"|"1080p29.97"|"1080p50"|"1080p59.94"|"720p59.94"|"720p50"|"720p23.976"|"720p24"}"#,
        enabled: always_enabled,
        run: run_set_bluray_video,
        journal: true,
    },
    CommandSpec {
        id: "disc.setCapacity",
        label: "容量を設定",
        menu: &["ディスク"],
        shortcut: None,
        params: r#"{"capacity": "dvd5"|"dvd9"|"bd25"|"bd50"|"bd100"|"bd128"|{"custom":{"bytes":n}}}"#,
        enabled: always_enabled,
        run: run_set_capacity,
        journal: true,
    },
    CommandSpec {
        id: "disc.setRegions",
        label: "リージョンを設定",
        menu: &["ディスク"],
        shortcut: None,
        params: r#"{"regions": [1..8]}"#,
        enabled: always_enabled,
        run: run_set_regions,
        journal: true,
    },
];

fn run_set_name(session: &mut Session, params: &Value) -> CommandResult {
    let name = req_str(params, "name")?.trim().to_owned();
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!(
            "the disc name is longer than {MAX_NAME_CHARS} characters"
        ));
    }
    session.project.settings.name = name;
    Ok(empty_object())
}

fn run_set_description(session: &mut Session, params: &Value) -> CommandResult {
    let description = req_str(params, "description")?;
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(format!(
            "the disc description is longer than {MAX_DESCRIPTION_CHARS} characters"
        ));
    }
    session.project.settings.description = description;
    Ok(empty_object())
}

/// The audio track a disc selects when it starts (`null`: the player's choice).
fn run_set_audio(session: &mut Session, params: &Value) -> CommandResult {
    let track = match params.get("track") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .filter(|n| (1..=8).contains(n))
                .ok_or_else(|| "parameter \"track\" must be 1 to 8 or null".to_string())?,
        ),
    };
    session.project.settings.audio_default = track;
    Ok(empty_object())
}

/// The language (ISO 639-1, two letters) of audio without its own; `null` (or empty) follows DiscCraft's display
/// language when the disc is built.
fn run_set_audio_language(session: &mut Session, params: &Value) -> CommandResult {
    let language = match params.get("language") {
        None | Some(Value::Null) => None,
        Some(Value::String(code)) if code.trim().is_empty() => None,
        Some(Value::String(code)) => {
            let code = code.trim().to_ascii_lowercase();
            if code.len() != 2 || !code.bytes().all(|b| b.is_ascii_lowercase()) {
                return Err(
                    "parameter \"language\" must be a two-letter ISO 639-1 code or null"
                        .to_string(),
                );
            }
            Some(code)
        }
        Some(_) => {
            return Err(
                "parameter \"language\" must be a two-letter ISO 639-1 code or null".to_string(),
            );
        }
    };
    session.project.settings.audio_language = language;
    Ok(empty_object())
}

/// The manual layer break of a dual-layer DVD: a timeline and a time in it (ticks, or a timecode in the disc's
/// format); `"timeline": null` goes back to the automatic one. The time must be within the timeline's chapters'
/// range start (0) and is taken at the frame nearest to it when the disc is built.
fn run_set_layer_break(session: &mut Session, params: &Value) -> CommandResult {
    let timeline = match params.get("timeline") {
        None | Some(Value::Null) => {
            session.project.settings.layer_break = None;
            return Ok(empty_object());
        }
        Some(value) => value
            .as_u64()
            .map(disccraft_project::TimelineId)
            .ok_or_else(|| "parameter \"timeline\" must be a timeline id or null".to_string())?,
    };
    if session.project.timeline(timeline).is_none() {
        return Err(format!("timeline {} does not exist", timeline.0));
    }
    let time = match (params.get("time"), params.get("timecode")) {
        (Some(value), _) => value
            .as_i64()
            .filter(|ticks| *ticks >= 0)
            .map(disccraft_project::Tick)
            .ok_or_else(|| "parameter \"time\" must be a tick count from 0".to_string())?,
        (None, Some(Value::String(text))) => {
            disccraft_project::parse_time(text.trim(), &session.project.settings)
                .map_err(|error| format!("parameter \"timecode\": {error}"))?
        }
        _ => return Err("give \"time\" (ticks) or \"timecode\"".to_string()),
    };
    session.project.settings.layer_break =
        Some(disccraft_project::LayerBreakPoint { timeline, time });
    Ok(empty_object())
}

/// The subtitle a disc selects when it starts (`null`: the player's choice; `"off"`: none).
fn run_set_subtitle(session: &mut Session, params: &Value) -> CommandResult {
    use disccraft_project::SubtitleDefault;
    let choice = match params.get("track") {
        None | Some(Value::Null) => SubtitleDefault::NoChange,
        Some(Value::String(text)) if text == "off" => SubtitleDefault::Off,
        Some(value) => SubtitleDefault::Stream(
            value
                .as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .filter(|n| (1..=32).contains(n))
                .ok_or_else(|| {
                    "parameter \"track\" must be 1 to 32, \"off\" or null".to_string()
                })?,
        ),
    };
    session.project.settings.subtitle_default = choice;
    Ok(empty_object())
}

fn run_set_user_ops(session: &mut Session, params: &Value) -> CommandResult {
    session.project.settings.prohibited_ops = super::user_ops_param(params)?;
    Ok(empty_object())
}

fn run_set_kind(session: &mut Session, params: &Value) -> CommandResult {
    let value = req_value(params, "kind")?;
    let kind: DiscKind = serde_json::from_value(value.clone())
        .map_err(|error| format!("parameter \"kind\" must be \"dvd\" or \"bluRay\": {error}"))?;

    let before = session.project.settings.clone();
    if session.project.settings.kind != kind {
        session.project.settings.kind = kind;
        session.project.settings.capacity = match kind {
            DiscKind::Dvd => Capacity::Dvd5,
            DiscKind::BluRay => Capacity::Bd25,
        };
    } else {
        session.project.settings.kind = kind;
    }
    // Blu-ray with HD titles has HD menus.
    session.project.retarget_menus(&before);

    Ok(empty_object())
}

fn run_set_standard(session: &mut Session, params: &Value) -> CommandResult {
    let value = req_value(params, "standard")?;
    let standard: VideoStandard = serde_json::from_value(value.clone())
        .map_err(|error| format!("parameter \"standard\" must be \"ntsc\" or \"pal\": {error}"))?;

    let before = session.project.settings.clone();
    session.project.settings.standard = standard;
    session.project.retarget_menus(&before);
    Ok(empty_object())
}

/// The disc's aspect ratio, which every title takes: the timelines follow it again (one can still be given its own
/// afterwards with `timeline.setAspect`), and slideshows always do. Menus keep theirs.
fn run_set_aspect(session: &mut Session, params: &Value) -> CommandResult {
    let aspect = req_value(params, "aspect")?.as_str().unwrap_or_default();
    session.project.settings.widescreen = match aspect {
        "4:3" => false,
        "16:9" => true,
        _ => return Err("parameter \"aspect\" must be \"4:3\" or \"16:9\"".to_string()),
    };
    for timeline in session.project.timelines.values_mut() {
        timeline.widescreen = None;
    }
    Ok(empty_object())
}

/// The video of a Blu-ray disc's titles: SD MPEG-2, or an HD H.264 format.
fn run_set_bluray_video(session: &mut Session, params: &Value) -> CommandResult {
    let id = crate::params::req_str(params, "video")?;
    let video = disccraft_project::BdVideo::from_id(&id).ok_or_else(|| {
        format!(
            "parameter \"video\" must be one of {}",
            disccraft_project::BdVideo::ALL
                .iter()
                .map(|video| video.id())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let before = session.project.settings.clone();
    session.project.settings.bd_video = video;
    // The menus move onto the HD frame (or back).
    session.project.retarget_menus(&before);
    Ok(empty_object())
}

fn run_set_capacity(session: &mut Session, params: &Value) -> CommandResult {
    let value = req_value(params, "capacity")?;
    let capacity: Capacity = serde_json::from_value(value.clone())
        .map_err(|error| format!("parameter \"capacity\" must be a valid capacity: {error}"))?;

    session.project.settings.capacity = capacity;
    Ok(empty_object())
}

fn run_set_regions(session: &mut Session, params: &Value) -> CommandResult {
    let value = req_value(params, "regions")?;
    let items = value
        .as_array()
        .ok_or_else(|| "parameter \"regions\" must be an array".to_string())?;

    let mut regions = Vec::new();
    for item in items {
        let number = item
            .as_u64()
            .ok_or_else(|| "parameter \"regions\" must contain integers".to_string())?;
        let region =
            u8::try_from(number).map_err(|_| "DVD region must be between 1 and 8".to_string())?;

        if !(1..=8).contains(&region) {
            return Err("DVD region must be between 1 and 8".to_string());
        }

        regions.push(region);
    }

    regions.sort_unstable();
    regions.dedup();
    session.project.settings.regions = regions;

    Ok(empty_object())
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use serde_json::json;
    use std::error::Error;

    /// M14.2: the manual layer break takes a timeline and a time (ticks or a timecode), checks them, goes back to
    /// automatic with `null`, and undoes.
    #[test]
    fn the_layer_break_is_set_checked_and_undone() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        let asset = session
            .project
            .add_asset(std::path::Path::new("movie.m2v"))?;
        let timeline = session.project.add_timeline(asset, None)?;
        session.execute(
            "disc.setLayerBreak",
            &json!({"timeline": timeline.0, "timecode": "00;00;01;07"}),
        )?;
        let point = session
            .project
            .settings
            .layer_break
            .ok_or("no layer break")?;
        assert_eq!(point.timeline, timeline);
        // 1 s and 7 frames of NTSC drop-frame timecode: 37 frames of 1001/30000 s.
        assert_eq!(point.time.0, 37 * 8_475_667_200);
        session.execute(
            "disc.setLayerBreak",
            &json!({"timeline": timeline.0, "time": 5}),
        )?;
        assert_eq!(
            session.project.settings.layer_break.map(|p| p.time.0),
            Some(5)
        );
        for bad in [
            json!({"timeline": 999, "time": 0}),
            json!({"timeline": timeline.0, "time": -1}),
            json!({"timeline": timeline.0}),
            json!({"timeline": timeline.0, "timecode": "nonsense"}),
        ] {
            assert!(
                session.execute("disc.setLayerBreak", &bad).is_err(),
                "{bad}"
            );
        }
        session.execute("disc.setLayerBreak", &json!({"timeline": null}))?;
        assert_eq!(session.project.settings.layer_break, None);
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(
            session.project.settings.layer_break.map(|p| p.time.0),
            Some(5)
        );
        Ok(())
    }

    #[test]
    fn the_audio_language_is_set_checked_and_undone() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        // Unset: the display language decides at build time (and nothing goes into the project file).
        assert_eq!(session.project.settings.audio_language, None);
        let text = serde_json::to_string(&session.project.settings)?;
        assert!(!text.contains("audioLanguage"), "{text}");

        session.execute("disc.setAudioLanguage", &json!({"language": " EN "}))?;
        assert_eq!(
            session.project.settings.audio_language.as_deref(),
            Some("en")
        );
        let text = serde_json::to_string(&session.project.settings)?;
        assert!(text.contains("\"audioLanguage\":\"en\""), "{text}");
        for bad in [
            json!({"language": "eng"}),
            json!({"language": "j1"}),
            json!({"language": 7}),
        ] {
            assert!(session.execute("disc.setAudioLanguage", &bad).is_err());
        }
        session.execute("disc.setAudioLanguage", &json!({"language": null}))?;
        assert_eq!(session.project.settings.audio_language, None);
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(
            session.project.settings.audio_language.as_deref(),
            Some("en")
        );
        Ok(())
    }

    #[test]
    fn the_blu_ray_video_is_set_and_undone() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        assert_eq!(
            session.project.settings.bd_video,
            disccraft_project::BdVideo::Sd
        );
        session.execute("disc.setBlurayVideo", &json!({"video": "1080p23.976"}))?;
        assert_eq!(
            session.project.settings.bd_video,
            disccraft_project::BdVideo::Hd1080p23
        );
        assert_eq!(
            session.project.settings.bd_video.hd(),
            Some((1920, 1080, 24_000, 1001))
        );
        // Kept in the project file.
        let text = serde_json::to_string(&session.project.settings)?;
        assert!(text.contains("\"bdVideo\":\"1080p23.976\""), "{text}");
        assert!(
            session
                .execute("disc.setBlurayVideo", &json!({"video": "1080i"}))
                .is_err()
        );
        session.execute("edit.undo", &json!({}))?;
        assert_eq!(
            session.project.settings.bd_video,
            disccraft_project::BdVideo::Sd
        );
        Ok(())
    }

    #[test]
    fn name_and_description_are_set_and_undone() -> Result<(), Box<dyn Error>> {
        let mut session = Session::new();
        session.execute("disc.setName", &json!({"name": "  My Disc  "}))?;
        session.execute(
            "disc.setDescription",
            &json!({"description": "line 1\nline 2"}),
        )?;
        assert_eq!(session.project.settings.name, "My Disc");
        assert_eq!(session.project.settings.description, "line 1\nline 2");

        // Too long, or not a string: refused and nothing changes.
        let long = "x".repeat(129);
        assert!(
            session
                .execute("disc.setName", &json!({"name": long}))
                .is_err()
        );
        assert!(
            session
                .execute("disc.setName", &json!({"name": 3}))
                .is_err()
        );
        let long = "x".repeat(4097);
        assert!(
            session
                .execute("disc.setDescription", &json!({"description": long}))
                .is_err()
        );
        assert_eq!(session.project.settings.name, "My Disc");

        session.execute("disc.setAudio", &json!({"track": 3}))?;
        session.execute("disc.setSubtitle", &json!({"track": "off"}))?;
        assert_eq!(session.project.settings.audio_default, Some(3));
        assert_eq!(
            session.project.settings.subtitle_default,
            disccraft_project::SubtitleDefault::Off
        );
        assert!(
            session
                .execute("disc.setAudio", &json!({"track": 9}))
                .is_err()
        );
        assert!(
            session
                .execute("disc.setSubtitle", &json!({"track": "on"}))
                .is_err()
        );
        session.execute("disc.setSubtitle", &json!({"track": 2}))?;
        session.execute("disc.setAudio", &json!({"track": null}))?;
        assert_eq!(session.project.settings.audio_default, None);
        session.undo()?;
        assert_eq!(session.project.settings.audio_default, Some(3));
        session.undo()?;
        session.undo()?;
        session.undo()?;

        session.execute("disc.setUserOps", &json!({"prohibited": ["stop"]}))?;
        assert_eq!(
            session.project.settings.prohibited_ops,
            vec![disccraft_project::UserOp::Stop]
        );
        session.undo()?;
        assert!(session.project.settings.prohibited_ops.is_empty());

        session.undo()?;
        assert_eq!(session.project.settings.description, "");
        session.undo()?;
        assert_eq!(session.project.settings.name, "");
        Ok(())
    }
}
