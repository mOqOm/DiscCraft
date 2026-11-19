//! Subtitles: the cues of a subtitle asset (for the timeline and the monitor), SRT export, and Encore's subtitle
//! editing in the Monitor (add a subtitle at the current time, trim its in / out point, change its text).
//!
//! A subtitle asset's cues come from its file until the first edit; the edit copies them into the project (the
//! file is left as it is) and from then on the project's cues are the asset's (`Asset::cues`).

use std::path::Path;

use crate::params::{opt_str, opt_value, req_str, req_u64, req_usize};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_menu::subtitle::guess_image_colors;
use disccraft_menu::subtitle_file::{
    ImageScriptCue, asset_cues, read_image_script, read_subtitles, write_srt, write_text_script,
};
use disccraft_project::{
    Asset, AssetId, AssetKind, CueImage, ImageCueColors, MAX_SUBTITLE_CUES, SubtitleCue, Tick,
};
use serde_json::{Value, json};

use super::always_enabled;

/// Ticks per second (`filmcraft-time`).
const TICKS_PER_SECOND: f64 = 254_016_000_000.0;

/// Subtitle commands.
pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "subtitle.cues",
        label: "字幕の一覧",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"asset": id}"#,
        enabled: always_enabled,
        run: run_cues,
        journal: false,
    },
    CommandSpec {
        id: "subtitle.addCue",
        label: "字幕を追加",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"asset": id, "start": ticks, "end"?: ticks, "text"?: string}"#,
        enabled: always_enabled,
        run: run_add_cue,
        journal: true,
    },
    CommandSpec {
        id: "subtitle.setCue",
        label: "字幕を変更",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"asset": id, "cue": index, "start"?: ticks, "end"?: ticks, "text"?: string, "group"?: 1-3 | null, "stroke"?: "none" | "thin" | "medium" | "thick" | "extraThick" | null, "align"?: 0-100 | null, "justify"?: "left" | "center" | "right" | null}"#,
        enabled: always_enabled,
        run: run_set_cue,
        journal: true,
    },
    CommandSpec {
        id: "subtitle.removeCue",
        label: "字幕を削除",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"asset": id, "cue": index}"#,
        enabled: always_enabled,
        run: run_remove_cue,
        journal: true,
    },
    CommandSpec {
        id: "subtitle.readImageScript",
        label: "画像スクリプトを読む",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"path": string, "fab"?: bool (a FAB image script), "folder"?: string (a FAB script's pictures; default: the script's folder)}"#,
        enabled: always_enabled,
        run: run_read_image_script,
        journal: false,
    },
    CommandSpec {
        id: "subtitle.importImageScript",
        label: "字幕の読み込み（画像スクリプト）",
        menu: &["タイムライン"],
        shortcut: None,
        params: r##"{"path": string, "fab"?: bool, "folder"?: string, "background"?: "#rrggbb", "fill"?: "#rrggbb", "outline"?: "#rrggbb" (default: guessed from the first picture), "offset"?: ticks (Relative: added to every timecode)}"##,
        enabled: always_enabled,
        run: run_import_image_script,
        journal: true,
    },
    CommandSpec {
        id: "subtitle.exportSrt",
        label: "字幕を書き出し",
        menu: &["タイムライン"],
        shortcut: None,
        params: r#"{"asset": id, "path": string (.txt: Encore's text script, otherwise SRT)}"#,
        enabled: always_enabled,
        run: run_export,
        journal: false,
    },
];

/// A subtitle added without an end lasts this long (or up to the next subtitle).
const DEFAULT_CUE_TICKS: i64 = 2 * 254_016_000_000;

/// The longest text one subtitle holds (characters).
const MAX_CUE_TEXT: usize = 2_000;

/// A subtitle asset.
fn subtitle_asset<'a>(session: &'a Session, params: &Value) -> Result<&'a Asset, String> {
    let id = req_u64(params, "asset")?;
    let asset = session
        .project
        .assets
        .get(&AssetId(id))
        .ok_or_else(|| format!("unknown asset: {id}"))?;
    if asset.kind != AssetKind::Subtitle {
        return Err("not a subtitle asset".to_string());
    }
    Ok(asset)
}

/// An optional time parameter (ticks).
fn opt_ticks(params: &Value, name: &str) -> Result<Option<Tick>, String> {
    opt_value(params, name)
        .map(|value| {
            value
                .as_i64()
                .map(Tick)
                .ok_or_else(|| format!("parameter \"{name}\" must be an integer"))
        })
        .transpose()
}

/// An optional subtitle text parameter (at most [`MAX_CUE_TEXT`] characters; `\r\n` becomes `\n`).
fn opt_text(params: &Value) -> Result<Option<String>, String> {
    let Some(text) = opt_str(params, "text")? else {
        return Ok(None);
    };
    if text.chars().count() > MAX_CUE_TEXT {
        return Err(format!(
            "a subtitle holds at most {MAX_CUE_TEXT} characters"
        ));
    }
    Ok(Some(text.replace("\r\n", "\n")))
}

/// The asset's cues to edit: the project's, or (on the first edit) its file's copied into the project.
fn editable_cues<'a>(
    session: &'a mut Session,
    params: &Value,
) -> Result<&'a mut Vec<SubtitleCue>, String> {
    let asset = subtitle_asset(session, params)?;
    let id = asset.id;
    let copied = match &asset.cues {
        Some(_) => None,
        None => Some(
            read_subtitles(Path::new(&asset.path), &session.project.settings)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|cue| SubtitleCue {
                    start: cue.start,
                    end: cue.end,
                    text: cue.text,
                    look: cue.look,
                    image: cue.image,
                })
                .collect(),
        ),
    };
    let asset = session
        .project
        .assets
        .get_mut(&id)
        .ok_or_else(|| format!("unknown asset: {}", id.0))?;
    Ok(asset.cues.get_or_insert_with(|| copied.unwrap_or_default()))
}

/// Adds a subtitle at `start` (Encore's Add Subtitle at the current time): it lasts until `end`, else two seconds
/// or up to the next subtitle. Refused where a subtitle is already shown or when it would run into the next one.
/// `{"cue": index, "start": ticks, "end": ticks}`.
fn run_add_cue(session: &mut Session, params: &Value) -> CommandResult {
    let start = opt_ticks(params, "start")?.ok_or("parameter \"start\" is required")?;
    let end = opt_ticks(params, "end")?;
    let text = opt_text(params)?.unwrap_or_default();
    if start.0 < 0 {
        return Err("a subtitle cannot start before zero".to_string());
    }
    let cues = editable_cues(session, params)?;
    if cues.len() >= MAX_SUBTITLE_CUES {
        return Err(format!(
            "a track holds at most {MAX_SUBTITLE_CUES} subtitles"
        ));
    }
    if cues
        .iter()
        .any(|cue| cue.start.0 <= start.0 && start.0 < cue.end.0)
    {
        return Err("a subtitle is already shown here".to_string());
    }
    let index = cues
        .iter()
        .position(|cue| cue.start.0 > start.0)
        .unwrap_or(cues.len());
    let next = cues.get(index).map(|cue| cue.start.0);
    let end = match end {
        Some(end) => {
            if end.0 <= start.0 {
                return Err("a subtitle must end after it starts".to_string());
            }
            if next.is_some_and(|next| end.0 > next) {
                return Err("the subtitle would run into the next one".to_string());
            }
            end
        }
        None => {
            let end = start.0.saturating_add(DEFAULT_CUE_TICKS);
            Tick(next.map_or(end, |next| end.min(next)))
        }
    };
    cues.insert(
        index,
        SubtitleCue {
            start,
            end,
            text,
            look: disccraft_project::CueLook::default(),
            image: None,
        },
    );
    Ok(json!({ "cue": index, "start": start.0, "end": end.0 }))
}

/// The look parameters of `subtitle.setCue`: for each of `"group"` (1 to 3), `"stroke"` and `"align"` (0 to 100),
/// `None` when it is not given (keep), `Some(None)` for `null` (the track's), `Some(Some(_))` for a value.
#[allow(clippy::type_complexity)]
fn look_params(
    params: &Value,
) -> Result<
    (
        Option<Option<u8>>,
        Option<Option<disccraft_project::SlideStroke>>,
        Option<Option<u8>>,
    ),
    String,
> {
    let group = match params.get("group") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(value) => Some(Some(
            value
                .as_u64()
                .and_then(|group| group.checked_sub(1))
                .and_then(|group| u8::try_from(group).ok())
                .filter(|group| *group < disccraft_project::SubtitleColorSet::GROUPS)
                .ok_or_else(|| "parameter \"group\" must be 1 to 3 or null".to_string())?,
        )),
    };
    let stroke = match params.get("stroke") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(value) => Some(Some(
            value
                .as_str()
                .and_then(disccraft_project::SlideStroke::parse)
                .ok_or_else(|| {
                    "parameter \"stroke\" must be none, thin, medium, thick, extraThick or null"
                        .to_string()
                })?,
        )),
    };
    let align = match params.get("align") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(value) => Some(Some(
            value
                .as_u64()
                .and_then(|percent| u8::try_from(percent).ok())
                .filter(|percent| *percent <= 100)
                .ok_or_else(|| "parameter \"align\" must be 0 to 100 or null".to_string())?,
        )),
    };
    Ok((group, stroke, align))
}

/// Changes a subtitle's in point, out point or text (Encore's Trim Subtitle In / Out Point to Here and typing in
/// the Monitor). It must still start before it ends, and a moved point must not run into its neighbour.
fn run_set_cue(session: &mut Session, params: &Value) -> CommandResult {
    let index = req_usize(params, "cue")?;
    let start = opt_ticks(params, "start")?;
    let end = opt_ticks(params, "end")?;
    let text = opt_text(params)?;
    let look = look_params(params)?;
    let justify = match params.get("justify") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(value) => Some(Some(
            value
                .as_str()
                .and_then(disccraft_project::SubtitleJustify::parse)
                .ok_or_else(|| {
                    "parameter \"justify\" must be left, center, right or null".to_string()
                })?,
        )),
    };
    let cues = editable_cues(session, params)?;
    let count = cues.len();
    let previous_end = index
        .checked_sub(1)
        .and_then(|before| cues.get(before))
        .map(|cue| cue.end.0);
    let next_start = index
        .checked_add(1)
        .and_then(|after| cues.get(after))
        .map(|cue| cue.start.0);
    let cue = cues
        .get_mut(index)
        .ok_or_else(|| format!("no subtitle {index} (the track has {count})"))?;
    // As in Encore, a picture subtitle is shown as it is drawn: it has no text to change.
    if text.is_some() && cue.image.is_some() {
        return Err("a picture subtitle has no text to change".to_string());
    }
    let new_start = start.unwrap_or(cue.start);
    let new_end = end.unwrap_or(cue.end);
    if new_start.0 < 0 {
        return Err("a subtitle cannot start before zero".to_string());
    }
    if new_end.0 <= new_start.0 {
        return Err("a subtitle must end after it starts".to_string());
    }
    if start.is_some_and(|start| start != cue.start)
        && previous_end.is_some_and(|previous| new_start.0 < previous)
    {
        return Err("the subtitle would run into the one before it".to_string());
    }
    if end.is_some_and(|end| end != cue.end) && next_start.is_some_and(|next| new_end.0 > next) {
        return Err("the subtitle would run into the next one".to_string());
    }
    cue.start = new_start;
    cue.end = new_end;
    if let Some(text) = text {
        cue.text = text;
    }
    if let Some(group) = look.0 {
        cue.look.group = group;
    }
    if let Some(stroke) = look.1 {
        cue.look.stroke = stroke;
    }
    if let Some(align) = look.2 {
        cue.look.align = align;
    }
    if let Some(justify) = justify {
        cue.look.justify = justify;
    }
    Ok(json!({ "cue": index, "start": new_start.0, "end": new_end.0 }))
}

/// Removes a subtitle.
fn run_remove_cue(session: &mut Session, params: &Value) -> CommandResult {
    let index = req_usize(params, "cue")?;
    let cues = editable_cues(session, params)?;
    if index >= cues.len() {
        return Err(format!(
            "no subtitle {index} (the track has {})",
            cues.len()
        ));
    }
    let _ = cues.remove(index);
    Ok(json!({ "cue": index }))
}

/// `{"cues": [{"start": seconds, "end": seconds, "text": string}], "edited": bool}`, sorted by start; `edited`
/// when the cues are the project's (edited in the Monitor) rather than the file's.
fn run_cues(session: &mut Session, params: &Value) -> CommandResult {
    let asset = subtitle_asset(session, params)?;
    let edited = asset.cues.is_some();
    let cues = asset_cues(asset, &session.project.settings).map_err(|error| error.to_string())?;
    let cues: Vec<Value> = cues
        .iter()
        .map(|cue| {
            json!({
                "start": cue.start.0 as f64 / TICKS_PER_SECOND,
                "end": cue.end.0 as f64 / TICKS_PER_SECOND,
                "text": cue.text,
                "group": cue.look.group.map(|group| u64::from(group) + 1),
                "stroke": cue.look.stroke.map(disccraft_project::SlideStroke::as_str),
                "align": cue.look.align,
                "justify": cue.look.justify.map(disccraft_project::SubtitleJustify::as_str),
                "image": cue.image.as_ref().and_then(|image| serde_json::to_value(image).ok()),
            })
        })
        .collect();
    Ok(json!({ "cues": cues, "edited": edited }))
}

/// `#rrggbb`.
fn hex(color: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}

/// Map Colors as parameters and results: `{"background", "fill", "outline"}` as `#rrggbb`.
fn colors_json(colors: ImageCueColors) -> Value {
    json!({
        "background": hex(colors.background),
        "fill": hex(colors.fill),
        "outline": hex(colors.outline),
    })
}

/// The subtitles of the image script `params` names (`"fab"`: a FAB image script, whose pictures are in
/// `"folder"`, else beside it).
fn image_script_cues(
    session: &Session,
    params: &Value,
) -> Result<(String, Vec<ImageScriptCue>), String> {
    let path = req_str(params, "path")?;
    let fab = match opt_value(params, "fab") {
        None => false,
        Some(value) => value
            .as_bool()
            .ok_or_else(|| "parameter \"fab\" must be true or false".to_string())?,
    };
    let folder = opt_str(params, "folder")?;
    let cues = read_image_script(
        Path::new(&path),
        fab,
        folder.as_deref().map(Path::new),
        &session.project.settings,
    )
    .map_err(|error| error.to_string())?;
    if cues.is_empty() {
        return Err(format!("{path}: no subtitles"));
    }
    Ok((path, cues))
}

/// The colours guessed from the first of `cues`' pictures that can be found.
fn guessed_colors(cues: &[ImageScriptCue]) -> Result<ImageCueColors, String> {
    let picture = cues
        .iter()
        .map(|cue| &cue.picture)
        .find(|picture| picture.is_file())
        .ok_or_else(|| "none of the script's pictures is found".to_string())?;
    guess_image_colors(picture).map_err(|error| error.to_string())
}

/// Reads an image script ahead of importing it (Encore shows Map Colors over its first picture):
/// `{"cues": count, "first": the first picture, "missing": [pictures not found, the first 20], "missingCount": n,
/// "colors": the colours guessed from the first picture found}`.
fn run_read_image_script(session: &mut Session, params: &Value) -> CommandResult {
    let (_, cues) = image_script_cues(session, params)?;
    let missing: Vec<String> = cues
        .iter()
        .filter(|cue| !cue.picture.is_file())
        .map(|cue| cue.picture.to_string_lossy().into_owned())
        .collect();
    let first = cues
        .iter()
        .find(|cue| cue.picture.is_file())
        .map(|cue| cue.picture.to_string_lossy().into_owned());
    Ok(json!({
        "cues": cues.len(),
        "first": first,
        "missing": missing.iter().take(20).collect::<Vec<_>>(),
        "missingCount": missing.len(),
        "colors": colors_json(guessed_colors(&cues)?),
    }))
}

/// Timeline ▸ Import Subtitles ▸ Image Script / FAB Image Script: the script becomes a subtitle asset whose
/// subtitles are its pictures, with the colours mapped as given (else guessed from the first picture). Every
/// picture must be found. `"offset"` (Encore's Relative) moves every subtitle; those that then end before zero are
/// left out. `{"asset": id, "cues": count, "colors": {...}}`.
fn run_import_image_script(session: &mut Session, params: &Value) -> CommandResult {
    let (path, cues) = image_script_cues(session, params)?;
    if let Some(missing) = cues.iter().find(|cue| !cue.picture.is_file()) {
        return Err(format!("picture not found: {}", missing.picture.display()));
    }
    let guessed = guessed_colors(&cues)?;
    let color = |name: &str, guess: [u8; 3]| -> Result<[u8; 3], String> {
        match opt_str(params, name)? {
            Some(text) => super::menu::parse_color(&text),
            None => Ok(guess),
        }
    };
    let colors = ImageCueColors {
        background: color("background", guessed.background)?,
        fill: color("fill", guessed.fill)?,
        outline: color("outline", guessed.outline)?,
    };
    let offset = opt_ticks(params, "offset")?.unwrap_or(Tick(0)).0;
    let subtitles: Vec<SubtitleCue> = cues
        .into_iter()
        .filter_map(|cue| {
            let end = cue.end.0.saturating_add(offset);
            (end > 0).then(|| SubtitleCue {
                start: Tick(cue.start.0.saturating_add(offset).max(0)),
                end: Tick(end),
                text: String::new(),
                look: disccraft_project::CueLook::default(),
                image: Some(CueImage {
                    path: cue.picture.to_string_lossy().into_owned(),
                    area: cue.area,
                    colors,
                }),
            })
        })
        .collect();
    if subtitles.is_empty() {
        return Err("every subtitle ends before the start".to_string());
    }
    if subtitles.len() > MAX_SUBTITLE_CUES {
        return Err(format!(
            "a track holds at most {MAX_SUBTITLE_CUES} subtitles"
        ));
    }
    let count = subtitles.len();
    let id = session
        .project
        .add_asset(Path::new(&path))
        .map_err(|error| error.to_string())?;
    let asset = session
        .project
        .assets
        .get_mut(&id)
        .ok_or_else(|| format!("unknown asset: {}", id.0))?;
    asset.kind = AssetKind::Subtitle;
    asset.cues = Some(subtitles);
    Ok(json!({ "asset": id.0, "cues": count, "colors": colors_json(colors) }))
}

/// Writes the asset's cues (as edited, or its file's in any supported format) to `path` as SRT in UTF-8.
fn run_export(session: &mut Session, params: &Value) -> CommandResult {
    let asset = subtitle_asset(session, params)?;
    let target = req_str(params, "path")?;
    let cues = asset_cues(asset, &session.project.settings).map_err(|error| error.to_string())?;
    // A `.txt` path asks for Encore's text script (what Encore exports and imports); anything else is SRT.
    let script = std::path::Path::new(&target)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("txt"));
    let bytes = if script {
        write_text_script(&cues, &session.project.settings)
    } else {
        write_srt(&cues).into_bytes()
    };
    std::fs::write(&target, bytes).map_err(|error| format!("{target}: {error}"))?;
    Ok(json!({ "path": target, "cues": cues.len() }))
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use serde_json::{Value, json};
    use std::error::Error;

    /// A subtitle's own colour group, stroke and place (Encore's subtitle clip properties): set, listed, and put
    /// back to the track's with `null`; values outside the set and the frame are refused.
    #[test]
    fn a_subtitle_takes_its_own_look() -> Result<(), Box<dyn std::error::Error>> {
        let file =
            std::env::temp_dir().join(format!("disccraft-engine-look-{}.srt", std::process::id()));
        std::fs::write(
            &file,
            "1
00:00:01,000 --> 00:00:03,000
Hello

",
        )?;
        let mut session = Session::default();
        let asset = session.project.add_asset(&file)?;
        let cues = |session: &mut Session| -> Result<Value, String> {
            session.execute("subtitle.cues", &json!({ "asset": asset.0 }))
        };
        session.execute(
            "subtitle.setCue",
            &json!({ "asset": asset.0, "cue": 0, "group": 3, "stroke": "thick", "align": 0 }),
        )?;
        let listed = cues(&mut session)?;
        let cue = listed
            .get("cues")
            .and_then(|cues| cues.get(0))
            .ok_or("no cue")?;
        assert_eq!(cue.get("group"), Some(&json!(3)));
        assert_eq!(cue.get("stroke"), Some(&json!("thick")));
        assert_eq!(cue.get("align"), Some(&json!(0)));
        session.execute(
            "subtitle.setCue",
            &json!({ "asset": asset.0, "cue": 0, "justify": "left" }),
        )?;
        let justified = cues(&mut session)?;
        assert_eq!(
            justified
                .get("cues")
                .and_then(|cues| cues.get(0))
                .and_then(|cue| cue.get("justify")),
            Some(&json!("left"))
        );
        assert!(
            session
                .execute(
                    "subtitle.setCue",
                    &json!({ "asset": asset.0, "cue": 0, "justify": "middle" })
                )
                .is_err()
        );
        for bad in [
            json!({ "group": 4 }),
            json!({ "align": 101 }),
            json!({ "stroke": "huge" }),
        ] {
            let mut params = json!({ "asset": asset.0, "cue": 0 });
            if let (Some(map), Some(bad)) = (params.as_object_mut(), bad.as_object()) {
                map.extend(bad.clone());
            }
            assert!(
                session.execute("subtitle.setCue", &params).is_err(),
                "{params}"
            );
        }
        session.execute(
            "subtitle.setCue",
            &json!({ "asset": asset.0, "cue": 0, "group": null, "stroke": null, "align": null }),
        )?;
        let listed = cues(&mut session)?;
        let cue = listed
            .get("cues")
            .and_then(|cues| cues.get(0))
            .ok_or("no cue")?;
        assert_eq!(cue.get("group"), Some(&Value::Null));
        assert_eq!(cue.get("stroke"), Some(&Value::Null));
        let _ = std::fs::remove_file(&file);
        Ok(())
    }

    /// Timeline ▸ Import Subtitles ▸ FAB Image Script / Image Script: the script is read ahead (count, first
    /// picture, guessed colours), then imported as a subtitle asset whose subtitles are pictures; the Relative
    /// offset moves them, given colours win over the guess, a missing picture is refused, and a picture subtitle
    /// has no text to change.
    #[test]
    fn image_scripts_are_imported_as_picture_subtitles() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!(
            "disccraft-engine-image-script-{}",
            std::process::id()
        ));
        let pictures = dir.join("pictures");
        std::fs::create_dir_all(&pictures)?;
        // 4 x 2: three white pixels on blue... and one black.
        for name in ["one.png", "two.png"] {
            let file = std::fs::File::create(pictures.join(name))?;
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 4, 2);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header()?.write_image_data(&[
                0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 255, //
                0, 0, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0,
            ])?;
        }
        let fab = dir.join("fab.txt");
        std::fs::write(
            &fab,
            "one.png 00;00;01;00 00;00;02;00 0 400 720 470\r\ntwo.png 00;00;03;00 00;00;04;00 0 0 720 480\r\n",
        )?;
        let script = dir.join("script.txt");
        std::fs::write(
            &script,
            "Display_Area (0 0 720 480)\nDirectory pictures\n1 00;00;01;00 00;00;02;00 one.png\n2 00;00;05;00 00;00;06;00 two.png\n",
        )?;
        let mut session = Session::new();

        let read = session.execute(
            "subtitle.readImageScript",
            &json!({ "path": fab, "fab": true, "folder": pictures }),
        )?;
        assert_eq!(read.get("cues").and_then(Value::as_u64), Some(2));
        assert_eq!(read.get("missingCount").and_then(Value::as_u64), Some(0));
        assert_eq!(
            read.get("colors"),
            Some(&json!({ "background": "#0000ff", "fill": "#ffffff", "outline": "#000000" }))
        );

        // Imported with a Relative offset of one second back.
        let second = 254_016_000_000_i64;
        let imported = session.execute(
            "subtitle.importImageScript",
            &json!({ "path": fab, "fab": true, "folder": pictures, "offset": -second, "outline": "#ff0000" }),
        )?;
        let asset = imported
            .get("asset")
            .and_then(Value::as_u64)
            .ok_or("asset")?;
        assert_eq!(imported.get("cues").and_then(Value::as_u64), Some(2));
        let cues = session.execute("subtitle.cues", &json!({ "asset": asset }))?;
        let first = cues.pointer("/cues/0").ok_or("cue")?;
        // 00;00;01;00 is 30 frames at 29.97 fps (1.001 s), one second before that is 0.001 s.
        let start = first.get("start").and_then(Value::as_f64).ok_or("start")?;
        assert!((start - 0.001).abs() < 1e-6, "{start}");
        assert_eq!(
            first.pointer("/image/area"),
            Some(&json!([0, 400, 720, 470]))
        );
        assert_eq!(
            first.pointer("/image/colors"),
            Some(
                &json!({ "background": [0, 0, 255], "fill": [255, 255, 255], "outline": [255, 0, 0] })
            )
        );
        assert!(
            first
                .pointer("/image/path")
                .and_then(Value::as_str)
                .is_some_and(|path| path.ends_with("one.png"))
        );
        assert!(
            session
                .execute(
                    "subtitle.setCue",
                    &json!({ "asset": asset, "cue": 0, "text": "x" })
                )
                .is_err()
        );
        // Its times can still change.
        session.execute(
            "subtitle.setCue",
            &json!({ "asset": asset, "cue": 0, "end": second / 2 }),
        )?;

        // An image script: the folder from its Directory line, beside the script.
        let imported = session.execute("subtitle.importImageScript", &json!({ "path": script }))?;
        assert_eq!(imported.get("cues").and_then(Value::as_u64), Some(2));
        // A picture that is not there is refused.
        std::fs::remove_file(pictures.join("two.png"))?;
        assert!(
            session
                .execute("subtitle.importImageScript", &json!({ "path": script }))
                .is_err()
        );
        let read = session.execute("subtitle.readImageScript", &json!({ "path": script }))?;
        assert_eq!(read.get("missingCount").and_then(Value::as_u64), Some(1));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn cues_and_export() -> Result<(), Box<dyn Error>> {
        let dir =
            std::env::temp_dir().join(format!("disccraft-engine-subs-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let vtt = dir.join("en.vtt");
        std::fs::write(&vtt, "WEBVTT\n\n00:01.000 --> 00:02.500\nHello\n")?;
        let mut session = Session::new();
        let imported = session.execute("asset.import", &json!({ "path": vtt }))?;
        let asset = imported
            .get("asset")
            .and_then(Value::as_u64)
            .ok_or("asset")?;

        let cues = session.execute("subtitle.cues", &json!({ "asset": asset }))?;
        let first = cues.get("cues").and_then(|cues| cues.get(0)).ok_or("cue")?;
        assert_eq!(first.get("text").and_then(Value::as_str), Some("Hello"));
        assert_eq!(first.get("end").and_then(Value::as_f64), Some(2.5));

        let srt = dir.join("en.srt");
        session.execute(
            "subtitle.exportSrt",
            &json!({ "asset": asset, "path": srt }),
        )?;
        assert!(std::fs::read_to_string(&srt)?.contains("00:00:01,000 --> 00:00:02,500"));

        // A .txt path writes Encore's text script: UTF-16 LE, `number start end text`.
        let script = dir.join("en.txt");
        session.execute(
            "subtitle.exportSrt",
            &json!({ "asset": asset, "path": script }),
        )?;
        let bytes = std::fs::read(&script)?;
        assert_eq!(bytes.get(..2), Some(&[0xFF, 0xFE][..]));
        let units: Vec<u16> = bytes
            .get(2..)
            .unwrap_or_default()
            .chunks(2)
            .filter_map(|pair| Some(u16::from_le_bytes([*pair.first()?, *pair.get(1)?])))
            .collect();
        assert_eq!(
            String::from_utf16(&units)?,
            "1 00;00;01;00 00;00;02;15 Hello\r\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    const SECOND: i64 = 254_016_000_000;

    /// (start, end, text) of each cue, in seconds.
    fn cues(session: &mut Session, asset: u64) -> Result<Vec<(f64, f64, String)>, Box<dyn Error>> {
        let value = session.execute("subtitle.cues", &json!({ "asset": asset }))?;
        Ok(value
            .get("cues")
            .and_then(Value::as_array)
            .ok_or("cues")?
            .iter()
            .filter_map(|cue| {
                Some((
                    cue.get("start")?.as_f64()?,
                    cue.get("end")?.as_f64()?,
                    cue.get("text")?.as_str()?.to_owned(),
                ))
            })
            .collect())
    }

    /// Encore's Monitor subtitle editing: a subtitle added at the current time lasts two seconds or up to the next
    /// one, its in and out points trim to a time, its text changes; each edit undoes; the file is left alone and
    /// the edited cues are what the asset gives (and exports) from then on.
    #[test]
    fn subtitles_are_added_trimmed_and_typed() -> Result<(), Box<dyn Error>> {
        let dir =
            std::env::temp_dir().join(format!("disccraft-engine-edit-subs-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let srt = dir.join("ja.srt");
        let original =
            "1\n00:00:01,000 --> 00:00:02,000\nOne\n\n2\n00:00:05,000 --> 00:00:06,000\nTwo\n";
        std::fs::write(&srt, original)?;
        let mut session = Session::new();
        let imported = session.execute("asset.import", &json!({ "path": srt }))?;
        let asset = imported
            .get("asset")
            .and_then(Value::as_u64)
            .ok_or("asset")?;
        let edited = |session: &mut Session| -> Result<bool, Box<dyn Error>> {
            let value = session.execute("subtitle.cues", &json!({ "asset": asset }))?;
            Ok(value.get("edited").and_then(Value::as_bool) == Some(true))
        };
        assert!(!edited(&mut session)?);

        // At 4 s: two seconds would run into "Two" at 5 s, so it ends there.
        let added = session.execute(
            "subtitle.addCue",
            &json!({ "asset": asset, "start": 4 * SECOND }),
        )?;
        assert_eq!(added.get("cue").and_then(Value::as_u64), Some(1));
        assert_eq!(
            cues(&mut session, asset)?,
            vec![
                (1.0, 2.0, "One".to_owned()),
                (4.0, 5.0, String::new()),
                (5.0, 6.0, "Two".to_owned())
            ]
        );
        assert!(edited(&mut session)?);
        assert_eq!(std::fs::read_to_string(&srt)?, original);
        // Not where a subtitle is shown, nor running into the next.
        assert!(
            session
                .execute(
                    "subtitle.addCue",
                    &json!({ "asset": asset, "start": SECOND * 3 / 2 })
                )
                .is_err()
        );
        assert!(
            session
                .execute(
                    "subtitle.addCue",
                    &json!({ "asset": asset, "start": 3 * SECOND, "end": 9 * SECOND / 2 })
                )
                .is_err()
        );

        // Type it, trim its in point to 4.5 s and the first one's out point to 1.5 s.
        session.execute(
            "subtitle.setCue",
            &json!({ "asset": asset, "cue": 1, "text": "Hello\r\nthere", "start": 9 * SECOND / 2 }),
        )?;
        session.execute(
            "subtitle.setCue",
            &json!({ "asset": asset, "cue": 0, "end": 3 * SECOND / 2 }),
        )?;
        assert_eq!(
            cues(&mut session, asset)?,
            vec![
                (1.0, 1.5, "One".to_owned()),
                (4.5, 5.0, "Hello\nthere".to_owned()),
                (5.0, 6.0, "Two".to_owned())
            ]
        );
        // An in point at or after the out point, or an out point into the next subtitle, is refused.
        for bad in [
            json!({ "asset": asset, "cue": 1, "start": 5 * SECOND }),
            json!({ "asset": asset, "cue": 1, "end": 11 * SECOND / 2 }),
            json!({ "asset": asset, "cue": 2, "start": 9 * SECOND / 2 }),
            json!({ "asset": asset, "cue": 9, "text": "x" }),
        ] {
            assert!(session.execute("subtitle.setCue", &bad).is_err(), "{bad}");
        }

        // Export writes the edited cues.
        let out = dir.join("out.srt");
        session.execute(
            "subtitle.exportSrt",
            &json!({ "asset": asset, "path": out }),
        )?;
        let written = std::fs::read_to_string(&out)?.replace("\r\n", "\n");
        assert!(
            written.contains("00:00:04,500 --> 00:00:05,000\nHello\nthere"),
            "{written}"
        );

        // Remove, then undo everything back to the file's cues.
        session.execute("subtitle.removeCue", &json!({ "asset": asset, "cue": 2 }))?;
        assert_eq!(cues(&mut session, asset)?.len(), 2);
        for _ in 0..4 {
            session.execute("edit.undo", &json!({}))?;
        }
        assert!(!edited(&mut session)?);
        assert_eq!(cues(&mut session, asset)?.len(), 2);
        assert_eq!(
            cues(&mut session, asset)?.get(1).map(|cue| cue.0),
            Some(5.0)
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
