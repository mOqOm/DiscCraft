use crate::params::{req_str, req_u64};
use crate::{CommandResult, CommandSpec, Session};
use disccraft_project::{AssetId, AssetKind};
use serde_json::{Map, Number, Value};
use std::path::Path;

use super::{always_enabled, empty_object, object};

/// Command specifications for asset operations.
pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "asset.import",
        label: "アセットをインポート",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"path": string}"#,
        enabled: always_enabled,
        run: run_import,
        journal: true,
    },
    CommandSpec {
        id: "asset.setPath",
        label: "アセットのファイルを置き換え",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"asset": id, "path": "file"}"#,
        enabled: always_enabled,
        run: run_set_path,
        journal: true,
    },
    CommandSpec {
        id: "asset.list",
        label: "アセット一覧",
        menu: &["アセット"],
        shortcut: None,
        params: "{}",
        enabled: always_enabled,
        run: run_list,
        journal: false,
    },
    CommandSpec {
        id: "asset.compositions",
        label: "EffectCraft のプロジェクトのコンポジション",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"asset": id}"#,
        enabled: always_enabled,
        run: run_compositions,
        journal: false,
    },
    CommandSpec {
        id: "asset.setComposition",
        label: "アセットのコンポジションを選ぶ",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"asset": id, "composition": string | null}"#,
        enabled: always_enabled,
        run: run_set_composition,
        journal: true,
    },
    CommandSpec {
        id: "asset.pending",
        label: "読み込み中のアセットを調べる",
        menu: &["アセット"],
        shortcut: None,
        params: "{}",
        enabled: always_enabled,
        run: run_pending,
        journal: false,
    },
    CommandSpec {
        id: "asset.probe",
        label: "アセットの長さを調べる",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"asset": id, "wait": bool (既定 true。false: まだ開いていなければ裏で開き {"pending": true} を返す)}"#,
        enabled: always_enabled,
        run: run_probe,
        journal: false,
    },
    CommandSpec {
        id: "asset.thumbnail",
        label: "アセットのサムネールを作る",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"asset": id}"#,
        enabled: always_enabled,
        run: run_thumbnail,
        journal: false,
    },
    CommandSpec {
        id: "asset.frame",
        label: "アセットのフレームを取り出す",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"asset": id, "seconds": f64, "bright"?: bool, "bars"?: bool, "wait"?: bool (既定 true。false: まだ開いていなければ裏で開き {"pending": true} を返す)}"#,
        enabled: always_enabled,
        run: run_frame,
        journal: false,
    },
];

/// モニターに映すフレームの最大辺（ピクセル）。
const FRAME_EDGE: u32 = 720;

fn run_frame(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "asset")?;
    let seconds = params
        .get("seconds")
        .and_then(Value::as_f64)
        .ok_or_else(|| "parameter \"seconds\" must be a number".to_string())?;
    let asset = session
        .project
        .assets
        .get(&AssetId(id))
        .ok_or_else(|| format!("asset.frame: unknown asset: {id}"))?;
    if asset.kind != AssetKind::Video {
        return Err("asset.frame: not a video asset".to_string());
    }

    // "bright": the first bright (non-black) frame from there (a still video thumbnail without a poster frame).
    // "bars": false gives the picture without its letterbox bars (a video thumbnail stretches it over its area).
    let bright = matches!(params.get("bright"), Some(Value::Bool(true)));
    let bars = !matches!(params.get("bars"), Some(Value::Bool(false)));
    let media = asset.media_file();
    let path: &Path = &media;
    if !wait(params) && !disccraft_transcode::media_open(path) {
        disccraft_transcode::warm(path);
        return Ok(pending(id));
    }
    let reader = disccraft_transcode::FrameReader::open(path).map_err(|error| error.to_string())?;
    let seconds = if bright {
        reader
            .first_bright_time(seconds)
            .map_err(|error| error.to_string())?
    } else {
        seconds
    };
    let (width, height, rgb) = if bars {
        reader.frame(seconds)
    } else {
        reader.picture(seconds)
    }
    .map_err(|error| error.to_string())?;
    let (width, height, rgb) = downscale_rgb(width, height, &rgb, 3, FRAME_EDGE)
        .ok_or_else(|| "asset.frame: cannot scale the frame".to_string())?;

    let bytes: Vec<Value> = rgb.into_iter().map(Value::from).collect();
    Ok(object(&[
        ("asset", Value::from(id)),
        ("seconds", Value::from(seconds)),
        ("width", Value::from(width)),
        ("height", Value::from(height)),
        ("rgb", Value::Array(bytes)),
    ]))
}

/// サムネールの最大辺（ピクセル）。
const THUMBNAIL_EDGE: u32 = 128;
/// The name video thumbnails are remembered under (`disccraft_transcode::cached_blob`).
const THUMBNAIL_BLOB: &str = "thumb";

/// What is remembered in place of a thumbnail for a video this build could not take a picture from.
fn no_thumbnail_mark() -> Vec<u8> {
    format!("no:{}", disccraft_transcode::build_tag()).into_bytes()
}

/// Whether the caller waits for a file that is not open yet (`"wait": false` does not).
fn wait(params: &Value) -> bool {
    !matches!(params.get("wait"), Some(Value::Bool(false)))
}

/// The answer for an asset whose file is being opened on its own thread: ask again later.
fn pending(id: u64) -> Value {
    object(&[("asset", Value::from(id)), ("pending", Value::Bool(true))])
}

/// A remembered thumbnail (width, height, RGB24); `None` for the mark of a video that gave no picture.
fn thumbnail_from_blob(id: u64, blob: &[u8]) -> Option<Value> {
    let width = u32::from_le_bytes(blob.get(0..4)?.try_into().ok()?);
    let height = u32::from_le_bytes(blob.get(4..8)?.try_into().ok()?);
    let rgb = blob.get(8..)?;
    let expected = usize::try_from(
        u64::from(width)
            .checked_mul(u64::from(height))?
            .checked_mul(3)?,
    )
    .ok()?;
    if width == 0 || height == 0 || rgb.len() != expected {
        return None;
    }
    Some(object(&[
        ("asset", Value::from(id)),
        ("width", Value::from(width)),
        ("height", Value::from(height)),
        (
            "rgb",
            Value::Array(rgb.iter().copied().map(Value::from).collect()),
        ),
    ]))
}

/// The video and audio assets whose files are still being opened (each is opened on a thread of its own from
/// here on): a long file takes seconds to open, and the interface asks about it only once this is empty.
/// `"opening"` lists those being opened whose length and size are known already (from the file's header, or
/// remembered): only their pictures are waited for.
fn run_pending(session: &mut Session, _params: &Value) -> CommandResult {
    let mut ids = Vec::new();
    let mut opening = Vec::new();
    for asset in session.project.assets.values() {
        if !matches!(asset.kind, AssetKind::Video | AssetKind::Audio) {
            continue;
        }
        let media = asset.media_file();
        let path: &Path = &media;
        if !disccraft_transcode::probe_ready(path) {
            disccraft_transcode::warm(path);
            ids.push(Value::from(asset.id.0));
        } else if disccraft_transcode::warming(path) {
            opening.push(Value::from(asset.id.0));
        }
    }
    Ok(object(&[
        ("pending", Value::Array(ids)),
        ("opening", Value::Array(opening)),
    ]))
}

fn run_thumbnail(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "asset")?;
    let asset = session
        .project
        .assets
        .get(&AssetId(id))
        .ok_or_else(|| format!("asset.thumbnail: unknown asset: {id}"))?;

    let media = asset.media_file();
    let path: &Path = &media;
    let video = asset.kind == AssetKind::Video;
    if video {
        // Made once and remembered between runs (a long file takes seconds to open). A failure is remembered
        // too, but only as this build's: a newer build's decoder may read the file, so it tries again.
        if let Some(blob) = disccraft_transcode::cached_blob(THUMBNAIL_BLOB, path) {
            if let Some(thumbnail) = thumbnail_from_blob(id, &blob) {
                return Ok(thumbnail);
            }
            if blob == no_thumbnail_mark() {
                return Err("asset.thumbnail: the video has no picture to show".to_string());
            }
        }
        if !wait(params) && !disccraft_transcode::media_open(path) {
            disccraft_transcode::warm(path);
            return Ok(pending(id));
        }
    }
    let (width, height, data, stride) = match asset.kind {
        AssetKind::Video => {
            let (width, height, rgb) = disccraft_transcode::first_frame(path).map_err(|error| {
                disccraft_transcode::store_blob(THUMBNAIL_BLOB, path, &no_thumbnail_mark());
                error.to_string()
            })?;
            (width, height, rgb, 3_usize)
        }
        AssetKind::Image => {
            // A vector drawing is rendered at thumbnail size instead of being scaled from its own size.
            let image = if disccraft_photo::is_svg(path) {
                let edge = THUMBNAIL_EDGE.saturating_mul(2);
                disccraft_photo::render_svg_fit(path, edge, edge)
            } else {
                disccraft_photo::load_image(path)
            }
            .map_err(|error| error.to_string())?;
            (image.width, image.height, image.rgba, 4_usize)
        }
        _ => return Err("asset.thumbnail: not a video or image asset".to_string()),
    };

    let (width, height, rgb) = downscale_rgb(width, height, &data, stride, THUMBNAIL_EDGE)
        .ok_or_else(|| "asset.thumbnail: cannot downscale the frame".to_string())?;
    if video {
        let mut blob = Vec::with_capacity(rgb.len().saturating_add(8));
        blob.extend_from_slice(&width.to_le_bytes());
        blob.extend_from_slice(&height.to_le_bytes());
        blob.extend_from_slice(&rgb);
        disccraft_transcode::store_blob(THUMBNAIL_BLOB, path, &blob);
    }

    let bytes: Vec<Value> = rgb.into_iter().map(Value::from).collect();
    Ok(object(&[
        ("asset", Value::from(id)),
        ("width", Value::from(width)),
        ("height", Value::from(height)),
        ("rgb", Value::Array(bytes)),
    ]))
}

/// 最近傍で `max_edge` に収まるよう縮小した RGB24（拡大はしない）。`stride` は 3 (RGB) か 4 (RGBA)。
fn downscale_rgb(
    width: u32,
    height: u32,
    data: &[u8],
    stride: usize,
    max_edge: u32,
) -> Option<(u32, u32, Vec<u8>)> {
    if width == 0 || height == 0 || max_edge == 0 {
        return None;
    }

    let (dst_w, dst_h) = if width >= height {
        let dst_w = width.min(max_edge);
        let dst_h = scaled(height, dst_w, width)?;
        (dst_w, dst_h)
    } else {
        let dst_h = height.min(max_edge);
        let dst_w = scaled(width, dst_h, height)?;
        (dst_w, dst_h)
    };

    let capacity = usize::try_from(
        u64::from(dst_w)
            .checked_mul(u64::from(dst_h))?
            .checked_mul(3)?,
    )
    .ok()?;
    let mut out = Vec::with_capacity(capacity);
    for y in 0..dst_h {
        let src_y = u64::from(y).checked_mul(u64::from(height))? / u64::from(dst_h);
        for x in 0..dst_w {
            let src_x = u64::from(x).checked_mul(u64::from(width))? / u64::from(dst_w);
            let index = src_y
                .checked_mul(u64::from(width))?
                .checked_add(src_x)?
                .checked_mul(u64::try_from(stride).ok()?)?;
            let base = usize::try_from(index).ok()?;
            out.push(*data.get(base)?);
            out.push(*data.get(base.checked_add(1)?)?);
            out.push(*data.get(base.checked_add(2)?)?);
        }
    }
    Some((dst_w, dst_h, out))
}

/// `value * numerator / denominator`（最低 1）。
fn scaled(value: u32, numerator: u32, denominator: u32) -> Option<u32> {
    let scaled =
        u64::from(value).checked_mul(u64::from(numerator))? / u64::from(denominator).max(1);
    u32::try_from(scaled.max(1)).ok()
}

/// Returns the stable name of an asset kind.
fn kind_name(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Video => "video",
        AssetKind::Audio => "audio",
        AssetKind::Image => "image",
        AssetKind::Subtitle => "subtitle",
        AssetKind::Other => "other",
    }
}

/// Imports an asset file into the project.
fn run_import(session: &mut Session, params: &Value) -> CommandResult {
    let path = req_str(params, "path")?;
    let file_path = Path::new(&path);

    if !file_path.is_file() {
        return Err(format!("asset.import: file does not exist: {path}"));
    }

    let id = session
        .project
        .add_asset(file_path)
        .map_err(|error| error.to_string())?;

    Ok(object(&[("asset", Value::from(id.0))]))
}

/// Points an asset at another file of the same kind (Encore's アセットを置き換え / アセットを検索).
fn run_set_path(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "asset")?;
    let path = req_str(params, "path")?;
    session
        .project
        .relink_asset(AssetId(id), Path::new(&path))
        .map_err(|error| error.to_string())?;
    Ok(empty_object())
}

/// Lists all assets in the project.
fn run_list(session: &mut Session, _params: &Value) -> CommandResult {
    let mut assets = Vec::new();

    for asset in session.project.assets.values() {
        let mut map = Map::new();
        map.insert("id".to_string(), Value::from(asset.id.0));
        map.insert("name".to_string(), Value::String(asset.name.clone()));
        map.insert("path".to_string(), Value::String(asset.path.clone()));
        map.insert(
            "kind".to_string(),
            Value::String(kind_name(asset.kind).to_string()),
        );
        map.insert("offline".to_string(), Value::Bool(asset.offline));
        assets.push(Value::Object(map));
    }

    Ok(Value::Array(assets))
}

/// The compositions of an EffectCraft project asset: their names, the one the project has active, and the one
/// chosen for the asset (`null`: the active one).
fn run_compositions(session: &mut Session, params: &Value) -> CommandResult {
    let id = AssetId(req_u64(params, "asset")?);
    let asset = session
        .project
        .assets
        .get(&id)
        .ok_or_else(|| format!("asset {} does not exist", id.0))?;
    let path = Path::new(&asset.path);
    if !disccraft_motion::is_project(path) {
        return Err("the asset is not an EffectCraft project".to_string());
    }
    let (names, active) =
        disccraft_motion::compositions(path).map_err(|error| error.to_string())?;
    let text = |name: &Option<String>| name.clone().map_or(Value::Null, Value::String);
    Ok(object(&[
        (
            "compositions",
            Value::Array(names.into_iter().map(Value::String).collect()),
        ),
        ("active", text(&active)),
        ("chosen", text(&asset.composition)),
    ]))
}

/// Chooses the composition an EffectCraft project asset shows (`null`: the project's active one).
fn run_set_composition(session: &mut Session, params: &Value) -> CommandResult {
    let id = AssetId(req_u64(params, "asset")?);
    let composition = match params.get("composition") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or_else(|| "parameter \"composition\" must be a string or null".to_string())?
                .to_owned(),
        ),
    };
    session
        .project
        .set_asset_composition(id, composition)
        .map_err(|error| error.to_string())?;
    session.sync_compositions();
    Ok(empty_object())
}

/// Probes the duration of a video or audio asset.
/// The length of a DTS file (`.dts`), which is not decoded but put on a DVD as it is: from its first frame and
/// its size.
fn dts_seconds(path: &Path) -> Option<f64> {
    use std::io::Read as _;
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("dts"))
    {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let total = file.metadata().ok()?.len();
    let mut head = [0u8; 64];
    let read = file.read(&mut head).ok()?;
    disccraft_psmux::es::dts::seconds_of(head.get(..read)?, total)
}

fn run_probe(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "asset")?;
    let asset = session
        .project
        .assets
        .get(&AssetId(id))
        .ok_or_else(|| format!("asset.probe: unknown asset: {id}"))?;

    if matches!(asset.kind, AssetKind::Video | AssetKind::Audio) && !wait(params) {
        let media = asset.media_file();
        let path: &Path = &media;
        if !disccraft_transcode::probe_ready(path) {
            disccraft_transcode::warm(path);
            return Ok(pending(id));
        }
    }

    let mut fields = serde_json::Map::new();
    let _ = fields.insert("asset".to_string(), Value::from(id));

    let mut seconds = None;
    match asset.kind {
        AssetKind::Video => {
            if let Ok(probe) = disccraft_transcode::probe_video(&asset.media_file()) {
                seconds = Some(probe.seconds);
                let _ = fields.insert("width".to_string(), Value::from(probe.width));
                let _ = fields.insert("height".to_string(), Value::from(probe.height));
                let _ = fields.insert("wide".to_string(), Value::from(probe.wide));
                let _ = fields.insert("audio".to_string(), Value::from(probe.has_audio));
                if let Some(fps) = Number::from_f64(probe.fps).filter(|_| probe.fps > 0.0) {
                    let _ = fields.insert("fps".to_string(), Value::Number(fps));
                }
            }
        }
        AssetKind::Audio => {
            seconds = dts_seconds(Path::new(&asset.path))
                .or_else(|| disccraft_transcode::probe_audio_seconds(&asset.media_file()).ok());
        }
        _ => {}
    }

    let seconds_value = seconds
        .and_then(Number::from_f64)
        .map_or(Value::Null, Value::Number);
    let _ = fields.insert("seconds".to_string(), seconds_value);

    Ok(Value::Object(fields))
}

/// Unit tests for asset commands.
#[cfg(test)]
mod tests {
    use super::run_probe;
    use crate::Session;
    use disccraft_project::{Asset, AssetId, AssetKind};
    use serde_json::{Value, json};

    /// An asset can be pointed at another file of the same kind; the name stays and it is online again.
    #[test]
    fn assets_are_relinked_to_files_of_the_same_kind() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-relink-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let first = dir.join("a.png");
        let second = dir.join("b.png");
        let audio = dir.join("c.wav");
        std::fs::write(&first, b"x")?;
        std::fs::write(&second, b"x")?;
        std::fs::write(&audio, b"x")?;

        let mut session = Session::default();
        let imported =
            session.execute("asset.import", &json!({"path": first.to_string_lossy()}))?;
        let id = imported
            .get("asset")
            .and_then(Value::as_u64)
            .ok_or("no asset id")?;
        session
            .project
            .assets
            .get_mut(&AssetId(id))
            .ok_or("missing asset")?
            .offline = true;

        session.execute(
            "asset.setPath",
            &json!({"asset": id, "path": second.to_string_lossy()}),
        )?;
        let asset = session
            .project
            .assets
            .get(&AssetId(id))
            .ok_or("missing asset")?;
        assert!(!asset.offline);
        assert_eq!(asset.name, "a");
        assert!(asset.path.ends_with("b.png"));

        // Another kind of media and a missing file are refused.
        assert!(
            session
                .execute(
                    "asset.setPath",
                    &json!({"asset": id, "path": audio.to_string_lossy()})
                )
                .is_err()
        );
        assert!(
            session
                .execute(
                    "asset.setPath",
                    &json!({"asset": id, "path": dir.join("missing.png").to_string_lossy()})
                )
                .is_err()
        );
        session.undo()?;
        let asset = session
            .project
            .assets
            .get(&AssetId(id))
            .ok_or("missing asset")?;
        assert!(asset.path.ends_with("a.png"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// Checks that probing an unknown asset id fails.
    #[test]
    fn probe_missing_asset_is_error() {
        let mut session = Session::default();
        let result = run_probe(&mut session, &json!({"asset": 999}));
        assert!(result.is_err());
    }

    /// Checks that probing an image asset returns a null duration.
    #[test]
    fn probe_image_asset_is_null() {
        let mut session = Session::default();
        let asset = Asset {
            id: AssetId(7),
            name: "image".to_string(),
            path: "image.png".to_string(),
            kind: AssetKind::Image,
            offline: false,
            cues: None,
            composition: None,
            description: String::new(),
        };
        let _ = session.project.assets.insert(AssetId(7), asset);

        let result = run_probe(&mut session, &json!({"asset": 7}));
        assert!(result.is_ok());

        let value = match result {
            Ok(value) => value,
            Err(_) => Value::Null,
        };
        assert!(matches!(value.get("seconds"), Some(Value::Null)));
    }

    /// M6.245: a video whose picture does not decode (interlaced H.264) is an error of the project check on a
    /// disc that would have to convert it, once the file is open; on a 1080i Blu-ray, which carries it as it is,
    /// it is none.
    #[test]
    fn a_video_that_cannot_be_converted_is_a_check_error() -> Result<(), Box<dyn std::error::Error>>
    {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/fixtures/h264_1080i.mkv");
        if !fixture.is_file() {
            eprintln!("h264_1080i.mkv fixture not found; skipping");
            return Ok(());
        }
        let dir =
            std::env::temp_dir().join(format!("disccraft-undecodable-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let video = dir.join("camera.mkv");
        std::fs::copy(&fixture, &video)?;
        let mut session = Session::new();
        let id = session
            .execute("asset.import", &json!({"path": video.to_string_lossy()}))?
            .get("asset")
            .and_then(Value::as_u64)
            .ok_or("no asset")?;
        let _ = session.execute("timeline.create", &json!({"asset": id}))?;
        let undecodable = |session: &mut Session| -> Result<bool, String> {
            let check = session.execute("project.check", &json!({}))?;
            Ok(check
                .get("problems")
                .and_then(Value::as_array)
                .is_some_and(|problems| {
                    problems
                        .iter()
                        .any(|problem| problem.get("code") == Some(&json!("undecodableVideo")))
                }))
        };
        // Wait until the file is open (its length is known at once from its header).
        let start = std::time::Instant::now();
        loop {
            let pending = session.execute("asset.pending", &json!({}))?;
            let _ = session.execute("asset.thumbnail", &json!({"asset": id, "wait": false}));
            if pending == json!({"pending": [], "opening": []}) && start.elapsed().as_millis() > 200
            {
                break;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(60));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(undecodable(&mut session)?, "a DVD would have to convert it");
        // A 1080i Blu-ray carries it as it is.
        session.execute("disc.setKind", &json!({"kind": "bluRay"}))?;
        session.execute("disc.setBlurayVideo", &json!({"video": "1080i59.94"}))?;
        assert!(!undecodable(&mut session)?);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// M12.13: asked without waiting, a video that is not open yet is opened on its own thread and answered
    /// "pending"; once `asset.pending` lists nothing, the same question is answered at once.
    #[test]
    fn a_video_is_opened_ahead_and_asked_without_waiting() -> Result<(), Box<dyn std::error::Error>>
    {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/fixtures/ntsc.m2v");
        if !fixture.is_file() {
            eprintln!("ntsc.m2v fixture not found; skipping");
            return Ok(());
        }
        // A copy of its own: nothing in this process has opened it.
        let dir = std::env::temp_dir().join(format!("disccraft-pending-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let video = dir.join("pending.m2v");
        std::fs::copy(&fixture, &video)?;
        let mut session = Session::new();
        let imported =
            session.execute("asset.import", &json!({"path": video.to_string_lossy()}))?;
        let id = imported
            .get("asset")
            .and_then(Value::as_u64)
            .ok_or("no asset")?;

        let first = session.execute("asset.probe", &json!({"asset": id, "wait": false}))?;
        assert_eq!(first.get("pending"), Some(&Value::Bool(true)), "{first}");
        let thumbnail = session.execute("asset.thumbnail", &json!({"asset": id, "wait": false}))?;
        assert_eq!(thumbnail.get("pending"), Some(&Value::Bool(true)));
        // The monitor's frame is not waited for either.
        let frame = json!({"asset": id, "seconds": 0.0, "wait": false});
        let waiting = session.execute("asset.frame", &frame)?;
        assert!(
            waiting.get("pending") == Some(&Value::Bool(true)) || waiting.get("rgb").is_some(),
            "{waiting}"
        );

        let start = std::time::Instant::now();
        loop {
            let pending = session.execute("asset.pending", &json!({}))?;
            if pending
                .get("pending")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
            {
                break;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(60));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let probe = session.execute("asset.probe", &json!({"asset": id, "wait": false}))?;
        assert_eq!(
            probe.get("width").and_then(Value::as_u64),
            Some(720),
            "{probe}"
        );
        assert!(probe.get("pending").is_none());
        let thumbnail = session.execute("asset.thumbnail", &json!({"asset": id, "wait": false}))?;
        assert!(thumbnail.get("rgb").is_some());
        let shown = session.execute("asset.frame", &frame)?;
        assert!(shown.get("rgb").is_some() && shown.get("pending").is_none());

        // A file that is not there is not waited for: the probe answers as before (no length).
        std::fs::remove_file(&video)?;
        let gone = session.execute("asset.probe", &json!({"asset": id, "wait": false}))?;
        assert!(gone.get("pending").is_none(), "{gone}");
        assert_eq!(
            session.execute("asset.pending", &json!({}))?,
            json!({"pending": [], "opening": []})
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
