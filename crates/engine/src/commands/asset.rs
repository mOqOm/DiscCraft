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
        id: "asset.probe",
        label: "アセットの長さを調べる",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"asset": id}"#,
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
        params: r#"{"asset": id, "seconds": f64, "bright"?: bool, "bars"?: bool}"#,
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
    let reader = disccraft_transcode::FrameReader::open(Path::new(&asset.path))
        .map_err(|error| error.to_string())?;
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

fn run_thumbnail(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "asset")?;
    let asset = session
        .project
        .assets
        .get(&AssetId(id))
        .ok_or_else(|| format!("asset.thumbnail: unknown asset: {id}"))?;

    let path = Path::new(&asset.path);
    let (width, height, data, stride) = match asset.kind {
        AssetKind::Video => {
            let (width, height, rgb) =
                disccraft_transcode::first_frame(path).map_err(|error| error.to_string())?;
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

/// Probes the duration of a video or audio asset.
fn run_probe(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "asset")?;
    let asset = session
        .project
        .assets
        .get(&AssetId(id))
        .ok_or_else(|| format!("asset.probe: unknown asset: {id}"))?;

    let mut fields = serde_json::Map::new();
    let _ = fields.insert("asset".to_string(), Value::from(id));

    let mut seconds = None;
    match asset.kind {
        AssetKind::Video => {
            if let Ok(probe) = disccraft_transcode::probe_video(Path::new(&asset.path)) {
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
            seconds = disccraft_transcode::probe_audio_seconds(Path::new(&asset.path)).ok();
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
}
