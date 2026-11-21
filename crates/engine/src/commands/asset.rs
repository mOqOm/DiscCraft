use crate::params::{opt_str, req_str, req_u64};
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
        id: "asset.locateTranscoded",
        label: "トランスコードされたファイルを検索",
        menu: &["ファイル", "トランスコードされたファイルを検索"],
        shortcut: None,
        params: r#"{"asset": id, "disc": "dvd" | "bluray", "video": string | null (null: forget), "audio"?: string}"#,
        enabled: always_enabled,
        run: run_locate_transcoded,
        journal: true,
    },
    CommandSpec {
        id: "asset.setTranscode",
        label: "トランスコード設定",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"asset": id, "kbps": number | null (null: 自動), "mode"?: "vbr" | "vbr2Pass" | "cbr" (VBR 1 パス / VBR 2 パス / CBR), "audio"?: "dolbyDigital" | "pcm" (DVD の音声), "preset"?: string | null (選んだ DVD の品質プリセットの名前), "film"?: bool (23.976 のフィルムで、3:2 プルダウンで見せる。NTSC の DVD の Encore の 23.976 のプリセット。自分のレートのときだけ), "quality"?: 1〜5 (Encore の品質。動きの探し方。High quality のプリセットは 5、Low quality は 2。自分のレートのときだけ。省略で今のまま、自動では 5)}"#,
        enabled: always_enabled,
        run: run_set_transcode,
        journal: true,
    },
    CommandSpec {
        id: "asset.setBlurayTranscode",
        label: "トランスコード設定（Blu-ray）",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"asset": id, "kbps": number | null (null: 自動), "maxKbps"?: number (最大。省略で kbps), "preset"?: string (品質プリセットの名前), "video"?: "1080p23.976" | "1080p24" | "1080i59.94" | "1080i50" | "1440p23.976" | "1440p24" | "1440i59.94" | "1440i50" | "720p59.94" | "720p50" | "720p23.976" | "720p24" | null (HD の Blu-ray でこのビデオのタイトルを作る形式。null: プロジェクト設定を使用（コーデックも）。省略で今のまま), "codec"?: "h264" | "mpeg2" | null (HD の Blu-ray でこのビデオのタイトルを作るコーデック。null: プロジェクトの。省略で今のまま), "quality"?: 1〜5 (MPEG-2 の品質。Long Form の MPEG-2 のプリセットは 3。省略で 5), "mode"?: "vbr" | "vbr2Pass" | "cbr" (レートの保ち方。Encore の Blu-ray のプリセットは VBR 1 パス。省略で vbr)}"#,
        enabled: always_enabled,
        run: run_set_bluray_transcode,
        journal: true,
    },
    CommandSpec {
        id: "asset.fitsBluray",
        label: "Blu-ray の形式にそのまま入るか",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"asset": id, "video": "sd" | "1080p23.976" | "1080p24" | "1080i59.94" | "1080i50" | "1440p23.976" | "1440p24" | "1440i59.94" | "1440i50" | "720p59.94" | "720p50" | "720p23.976" | "720p24"} → {"fits": bool (このビデオがその形式のタイトルにトランスコードせずに入るか)}"#,
        enabled: always_enabled,
        run: run_fits_bluray,
        journal: false,
    },
    CommandSpec {
        id: "asset.setRendering",
        label: "レンダリングの設定（最高のレンダリング品質・フレームブレンド）",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"asset": id, "maxRenderQuality"?: bool (最高のレンダリング品質を使用: 縮めるときは覆う画素の平均、広げるときは画素の中心のあいだで補間), "frameBlend"?: bool (フレームブレンドを使用: 出力の絵が素材の 2 つのフレームのあいだにあるときは近さで混ぜる)} (省略したものは今のまま)"#,
        enabled: always_enabled,
        run: run_set_rendering,
        journal: true,
    },
    CommandSpec {
        id: "asset.setIgnoreAlpha",
        label: "フッテージを変換（アルファチャンネルを無視）",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"asset": id, "ignore": bool}"#,
        enabled: always_enabled,
        run: run_set_ignore_alpha,
        journal: true,
    },
    CommandSpec {
        id: "asset.setPixelAspect",
        label: "フッテージを変換",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"asset": id, "aspect": [width, height] | null (null: ファイルのピクセル縦横比)}"#,
        enabled: always_enabled,
        run: run_set_pixel_aspect,
        journal: true,
    },
    CommandSpec {
        id: "asset.pixelAspect",
        label: "ピクセル縦横比",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"asset": id}"#,
        enabled: always_enabled,
        run: run_pixel_aspect,
        journal: false,
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
        params: r#"{"asset": id, "edge"?: 16..=1920 (長辺の上限。既定 128)}"#,
        enabled: always_enabled,
        run: run_thumbnail,
        journal: false,
    },
    CommandSpec {
        id: "asset.saveFrame",
        label: "フレームをファイルとして保存",
        menu: &["アセット"],
        shortcut: None,
        params: r#"{"asset": id, "seconds": f64, "path": "….png"}"#,
        enabled: always_enabled,
        run: run_save_frame,
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

/// Saves the frame of a video asset at `seconds` as a PNG file (Encore: Timeline ▸ Save Frame as File…): the
/// picture at its own size, in square pixels (a DVD-sized picture at 640 or 854 by its height, by its shape).
/// `{"width", "height"}` of the file written.
fn run_save_frame(session: &mut Session, params: &Value) -> CommandResult {
    let id = req_u64(params, "asset")?;
    let seconds = params
        .get("seconds")
        .and_then(Value::as_f64)
        .filter(|seconds| seconds.is_finite())
        .ok_or_else(|| "parameter \"seconds\" must be a number".to_string())?;
    let out = req_str(params, "path")?;
    let asset = session
        .project
        .assets
        .get(&AssetId(id))
        .ok_or_else(|| format!("asset.saveFrame: unknown asset: {id}"))?;
    if asset.kind != AssetKind::Video {
        return Err("asset.saveFrame: not a video asset".to_string());
    }
    let media = asset.media_file();
    let path: &Path = &media;
    let probe = disccraft_transcode::probe_video(path).map_err(|error| error.to_string())?;
    // Square pixels: a picture of a DVD's sizes is as wide as its shape makes it.
    let standard_definition =
        matches!(probe.width, 352 | 704 | 720) && matches!(probe.height, 480 | 576);
    let (width, height) = if standard_definition {
        let width = if probe.wide {
            probe.height.saturating_mul(16) / 9
        } else {
            probe.height.saturating_mul(4) / 3
        };
        (width, probe.height)
    } else {
        (probe.width, probe.height)
    };
    let reader = disccraft_transcode::FrameReader::open(path).map_err(|error| error.to_string())?;
    let (width, height, rgb) = reader
        .frame_sized(seconds.max(0.0), width.max(16), height.max(16))
        .map_err(|error| error.to_string())?;
    let file = std::fs::File::create(&out).map_err(|error| format!("{out}: {error}"))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| format!("{out}: {error}"))?;
    writer
        .write_image_data(&rgb)
        .map_err(|error| format!("{out}: {error}"))?;
    writer.finish().map_err(|error| format!("{out}: {error}"))?;
    Ok(object(&[
        ("width", Value::from(width)),
        ("height", Value::from(height)),
    ]))
}

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
    // A larger picture than a thumbnail (a slide shown in the preview) is made each time: only thumbnails are
    // remembered between runs.
    let edge = params
        .get("edge")
        .and_then(Value::as_u64)
        .and_then(|edge| u32::try_from(edge).ok())
        .map_or(THUMBNAIL_EDGE, |edge| edge.clamp(16, 1920));
    let video = asset.kind == AssetKind::Video && edge == THUMBNAIL_EDGE;
    if asset.kind == AssetKind::Video
        && !video
        && !wait(params)
        && !disccraft_transcode::media_open(path)
    {
        disccraft_transcode::warm(path);
        return Ok(pending(id));
    }
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
                if video {
                    disccraft_transcode::store_blob(THUMBNAIL_BLOB, path, &no_thumbnail_mark());
                }
                error.to_string()
            })?;
            (width, height, rgb, 3_usize)
        }
        AssetKind::Image => {
            // A vector drawing is rendered at thumbnail size instead of being scaled from its own size.
            let image = if disccraft_photo::is_svg(path) {
                let edge = edge.saturating_mul(2);
                disccraft_photo::render_svg_fit(path, edge, edge)
            } else {
                disccraft_photo::load_image(path)
            }
            .map_err(|error| error.to_string())?;
            let mut rgba = image.rgba;
            // A slide shown in the preview is as the disc shows it: over black (its alpha ignored: as it is).
            if edge != THUMBNAIL_EDGE && !asset.ignore_alpha {
                over_black(&mut rgba);
            }
            (image.width, image.height, rgba, 4_usize)
        }
        _ => return Err("asset.thumbnail: not a video or image asset".to_string()),
    };

    let (width, height, rgb) = downscale_rgb(width, height, &data, stride, edge)
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

/// RGBA `rgba` laid over black: each colour scaled by its alpha, every pixel opaque.
fn over_black(rgba: &mut [u8]) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        for channel in pixel.iter_mut().take(3) {
            *channel = u8::try_from((u16::from(*channel) * alpha + 127) / 255).unwrap_or(u8::MAX);
        }
        pixel[3] = 255;
    }
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
        // Transcoded files located for each kind of disc (Locate Transcoded File).
        for (key, located) in [
            ("locatedDvd", &asset.located_dvd),
            ("locatedBluray", &asset.located_bluray),
        ] {
            if let Some(value) = located
                .as_ref()
                .and_then(|located| serde_json::to_value(located).ok())
            {
                map.insert(key.to_string(), value);
            }
        }
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

/// Sets the pixel aspect ratio an asset's pictures are taken to have (Encore's Interpret Footage), or the
/// file's own with `null`.
fn run_set_pixel_aspect(session: &mut Session, params: &Value) -> CommandResult {
    let id = AssetId(req_u64(params, "asset")?);
    let aspect = match params.get("aspect") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let part = |index: usize| {
                value
                    .get(index)
                    .and_then(Value::as_u64)
                    .and_then(|part| u32::try_from(part).ok())
            };
            Some(part(0).zip(part(1)).ok_or_else(|| {
                "parameter \"aspect\" must be [width, height] in whole numbers, or null".to_string()
            })?)
        }
    };
    session
        .project
        .set_asset_pixel_aspect(id, aspect)
        .map_err(|error| error.to_string())?;
    session.sync_compositions();
    Ok(empty_object())
}

/// The pixel aspect ratio of an asset's pictures: `{"file": [width, height] | null, "chosen": [width, height] |
/// null}` (the file's own, `null` when the file cannot be opened as video, and the project's choice).
fn run_pixel_aspect(session: &mut Session, params: &Value) -> CommandResult {
    let id = AssetId(req_u64(params, "asset")?);
    let asset = session
        .project
        .assets
        .get(&id)
        .ok_or_else(|| format!("asset {} does not exist", id.0))?;
    let pair = |(width, height): (u32, u32)| serde_json::json!([width, height]);
    // A picture's own: what its file says, else Encore's for its size (a DVD-sized still has SD pixels).
    let own = if asset.kind == AssetKind::Image {
        disccraft_transcode::still_pixel_aspect(&asset.media_file())
    } else {
        disccraft_transcode::pixel_aspect(&asset.media_file())
    };
    let file = own.ok().map_or(Value::Null, pair);
    Ok(object(&[
        ("file", file),
        ("chosen", asset.pixel_aspect.map_or(Value::Null, pair)),
        ("ignoreAlpha", Value::Bool(asset.ignore_alpha)),
    ]))
}

/// Encore's Interpret Footage ▸ Ignore Alpha Channel for a picture: its pixels are all opaque in menus and slides.
fn run_set_ignore_alpha(session: &mut Session, params: &Value) -> CommandResult {
    let id = AssetId(req_u64(params, "asset")?);
    let ignore = params
        .get("ignore")
        .and_then(Value::as_bool)
        .ok_or_else(|| "parameter \"ignore\" must be true or false".to_string())?;
    let asset = session
        .project
        .assets
        .get_mut(&id)
        .ok_or_else(|| format!("asset {} does not exist", id.0))?;
    if asset.kind != AssetKind::Image {
        return Err(format!("asset {} is not a picture", id.0));
    }
    asset.ignore_alpha = ignore;
    Ok(empty_object())
}

/// Sets the video bit rate an asset is transcoded at (Encore's Transcode Settings), or automatic with `null`.
fn run_set_transcode(session: &mut Session, params: &Value) -> CommandResult {
    let id = AssetId(req_u64(params, "asset")?);
    let kbps = match params.get("kbps") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .and_then(|kbps| u32::try_from(kbps).ok())
                .ok_or_else(|| "parameter \"kbps\" must be a whole number or null".to_string())?,
        ),
    };
    let mode = opt_str(params, "mode")?
        .map(|text| {
            disccraft_project::TranscodeMode::parse(&text).ok_or_else(|| {
                "parameter \"mode\" must be \"vbr\", \"vbr2Pass\" or \"cbr\"".to_string()
            })
        })
        .transpose()?;
    session
        .project
        .set_asset_transcode(id, kbps)
        .map_err(|error| error.to_string())?;
    let audio = opt_str(params, "audio")?
        .map(|text| {
            disccraft_project::TranscodeAudio::parse(&text).ok_or_else(|| {
                "parameter \"audio\" must be \"dolbyDigital\" or \"pcm\"".to_string()
            })
        })
        .transpose()?;
    if let (Some(mode), Some(asset)) = (mode, session.project.assets.get_mut(&id)) {
        asset.transcode_mode = mode;
    }
    if let (Some(audio), Some(asset)) = (audio, session.project.assets.get_mut(&id)) {
        asset.transcode_audio = audio;
    }
    // The preset it was chosen as (Encore's DVD quality preset), only with a rate of its own; so is film.
    let preset = preset_name(params)?;
    let film = match params.get("film") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(film)) => Some(*film),
        Some(_) => return Err("parameter \"film\" must be true or false".to_string()),
    };
    let quality = quality_param(params)?;
    if let Some(asset) = session.project.assets.get_mut(&id) {
        asset.transcode_preset = preset.filter(|_| asset.transcode_kbps.is_some());
        asset.transcode_film =
            film.unwrap_or(asset.transcode_film) && asset.transcode_kbps.is_some();
        // The preset's quality (M6.326), only with a rate of its own: automatic is the best.
        asset.transcode_quality = if asset.transcode_kbps.is_some() {
            quality.unwrap_or(asset.transcode_quality)
        } else {
            disccraft_project::BEST_QUALITY
        };
    }
    Ok(empty_object())
}

/// The `quality` parameter: Encore's quality, [`disccraft_project::MIN_QUALITY`] to
/// [`disccraft_project::BEST_QUALITY`]; `None` when it is left out or null.
fn quality_param(params: &Value) -> Result<Option<u8>, String> {
    use disccraft_project::{BEST_QUALITY, MIN_QUALITY};
    match params.get("quality") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|quality| u8::try_from(quality).ok())
            .filter(|quality| (MIN_QUALITY..=BEST_QUALITY).contains(quality))
            .map(Some)
            .ok_or_else(|| {
                format!("parameter \"quality\" must be {MIN_QUALITY} to {BEST_QUALITY}, or null")
            }),
    }
}

/// The longest quality preset name, in characters.
const MAX_PRESET_NAME: usize = 120;

/// The `preset` parameter: a quality preset's name (trimmed, at most [`MAX_PRESET_NAME`] characters), `None`
/// when it is left out, null or empty.
fn preset_name(params: &Value) -> Result<Option<String>, String> {
    match params.get("preset") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(name)) => {
            let name: String = name.trim().chars().take(MAX_PRESET_NAME).collect();
            Ok((!name.is_empty()).then_some(name))
        }
        Some(_) => Err("parameter \"preset\" must be a name or null".to_string()),
    }
}

/// Encore's Transcode Settings ▸ Blu-ray ▸ Quality Preset: the rates a video asset is encoded at on a Blu-ray (its
/// target and peak), or automatic with `null`.
fn run_set_bluray_transcode(session: &mut Session, params: &Value) -> CommandResult {
    use disccraft_project::{MAX_TRANSCODE_KBPS, MIN_TRANSCODE_KBPS};
    let id = AssetId(req_u64(params, "asset")?);
    let rate = |name: &str| -> Result<Option<u32>, String> {
        match params.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_u64()
                .and_then(|kbps| u32::try_from(kbps).ok())
                .filter(|kbps| (MIN_TRANSCODE_KBPS..=MAX_TRANSCODE_KBPS).contains(kbps))
                .map(Some)
                .ok_or_else(|| {
                    format!(
                        "parameter \"{name}\" must be {MIN_TRANSCODE_KBPS} to {MAX_TRANSCODE_KBPS} kbit/s, or null"
                    )
                }),
        }
    };
    let kbps = rate("kbps")?;
    let max_kbps = rate("maxKbps")?;
    let preset = preset_name(params)?;
    let quality = quality_param(params)?.unwrap_or(disccraft_project::BEST_QUALITY);
    // How the rate is kept (M6.328): a preset of the user's may take two passes or keep it constant.
    let mode = opt_str(params, "mode")?
        .map(|text| {
            disccraft_project::TranscodeMode::parse(&text).ok_or_else(|| {
                "parameter \"mode\" must be \"vbr\", \"vbr2Pass\" or \"cbr\"".to_string()
            })
        })
        .transpose()?
        .unwrap_or_default();
    // Encore's Codec / Dimensions / Frame Rate (M6.325): an HD format of its own, or the project's with null.
    let video = match params.get("video") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(name)) => Some(Some(
            disccraft_project::BdVideo::from_id(name)
                .filter(|video| video.hd().is_some())
                .ok_or_else(|| format!("parameter \"video\": no HD Blu-ray format {name}"))?,
        )),
        Some(_) => return Err("parameter \"video\" must be an HD format or null".to_string()),
    };
    // Encore's Codec (M10.39): H.264 or MPEG-2 of its own, or the project's with null.
    let codec = match params.get("codec") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(name)) => Some(Some(
            disccraft_project::BdCodec::from_id(name)
                .filter(|codec| !codec.is_auto())
                .ok_or_else(|| format!("parameter \"codec\": no codec {name}"))?,
        )),
        Some(_) => return Err("parameter \"codec\" must be h264, mpeg2 or null".to_string()),
    };
    let asset = session
        .project
        .assets
        .get_mut(&id)
        .ok_or_else(|| format!("asset {} does not exist", id.0))?;
    if asset.kind != AssetKind::Video {
        return Err(format!("asset {} is not a video", id.0));
    }
    asset.bluray_transcode = kbps.map(|kbps| disccraft_project::BlurayTranscode {
        kbps,
        max_kbps: max_kbps.unwrap_or(kbps).max(kbps),
        preset: preset.unwrap_or_default(),
        quality,
        mode,
    });
    if let Some(video) = video {
        asset.bluray_video = video;
        // Use Project Settings: the project's codec too.
        if video.is_none() && codec.is_none() {
            asset.bluray_codec = None;
        }
    }
    if let Some(codec) = codec {
        asset.bluray_codec = codec;
    }
    Ok(empty_object())
}

/// Encore's Transcode Settings ▸ Use Maximum Render Quality / Use Frame Blending (M6.327): how a video asset's
/// transcodes make their pictures, for either kind of disc.
fn run_set_rendering(session: &mut Session, params: &Value) -> CommandResult {
    let id = AssetId(req_u64(params, "asset")?);
    let flag = |name: &str| -> Result<Option<bool>, String> {
        match params.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(on)) => Ok(Some(*on)),
            Some(_) => Err(format!("parameter \"{name}\" must be true or false")),
        }
    };
    let max_quality = flag("maxRenderQuality")?;
    let frame_blend = flag("frameBlend")?;
    let asset = session
        .project
        .assets
        .get_mut(&id)
        .ok_or_else(|| format!("asset {} does not exist", id.0))?;
    if asset.kind != AssetKind::Video {
        return Err(format!("asset {} is not a video", id.0));
    }
    if let Some(on) = max_quality {
        asset.max_render_quality = on;
    }
    if let Some(on) = frame_blend {
        asset.frame_blend = on;
    }
    Ok(empty_object())
}

/// Whether a video asset would go on a Blu-ray title in a format as it is (M6.325: the Transcode Settings dialog
/// shows "Don't Transcode" for a format being chosen only when it would).
fn run_fits_bluray(session: &mut Session, params: &Value) -> CommandResult {
    let id = AssetId(req_u64(params, "asset")?);
    let name = req_str(params, "video")?;
    let video = disccraft_project::BdVideo::from_id(&name)
        .ok_or_else(|| format!("parameter \"video\": no Blu-ray format {name}"))?;
    if !session.project.assets.contains_key(&id) {
        return Err(format!("asset {} does not exist", id.0));
    }
    let fits = disccraft_build::video_fits_bluray_as(&session.project, id, video);
    Ok(object(&[("fits", Value::from(fits))]))
}

/// Encore's Locate Transcoded File ▸ DVD… / Blu-ray…: the files a video asset was transcoded to for that kind of
/// disc, which a build of that kind then takes in place of the asset's own file (whose source may be gone). The
/// video must be a video file and the sound a sound file; `"video": null` forgets them (as Revert to Original).
fn run_locate_transcoded(session: &mut Session, params: &Value) -> CommandResult {
    let id = AssetId(req_u64(params, "asset")?);
    let kind = match req_str(params, "disc")?.as_str() {
        "dvd" => disccraft_project::DiscKind::Dvd,
        "bluray" => disccraft_project::DiscKind::BluRay,
        other => {
            return Err(format!(
                "parameter \"disc\" must be dvd or bluray, not {other}"
            ));
        }
    };
    let located = match params.get("video") {
        None => return Err("parameter \"video\" is required (a file, or null)".to_string()),
        Some(Value::Null) => None,
        Some(_) => {
            let video = req_str(params, "video")?;
            let file = |path: &str, wanted: AssetKind, what: &str| -> Result<String, String> {
                let found = Path::new(path);
                if !found.is_file() {
                    return Err(format!("{what} file does not exist: {path}"));
                }
                if AssetKind::guess_from_path(found) != wanted {
                    return Err(format!("not a {what} file: {path}"));
                }
                Ok(std::path::absolute(found)
                    .unwrap_or_else(|_| found.to_path_buf())
                    .to_string_lossy()
                    .into_owned())
            };
            Some(disccraft_project::LocatedTranscode {
                video: file(&video, AssetKind::Video, "video")?,
                audio: opt_str(params, "audio")?
                    .filter(|audio| !audio.trim().is_empty())
                    .map(|audio| file(&audio, AssetKind::Audio, "sound"))
                    .transpose()?,
            })
        }
    };
    let asset = session
        .project
        .assets
        .get_mut(&id)
        .ok_or_else(|| format!("unknown asset: {}", id.0))?;
    if asset.kind != AssetKind::Video {
        return Err(format!("asset {} is not a video", id.0));
    }
    let result = match &located {
        Some(found) => object(&[
            ("asset", Value::from(id.0)),
            ("video", Value::from(found.video.clone())),
            (
                "audio",
                found.audio.clone().map_or(Value::Null, Value::from),
            ),
        ]),
        None => object(&[("asset", Value::from(id.0)), ("video", Value::Null)]),
    };
    *asset.located_mut(kind) = located;
    Ok(result)
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
                // Encore's field type: progressive, or which field comes first (when the headers say).
                if let Some(field) = disccraft_transcode::field_type(&asset.media_file()) {
                    let _ = fields.insert("fieldType".to_string(), Value::from(field.as_str()));
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

    /// Locate Transcoded File: a video asset takes a video and a sound file for one kind of disc, listed with the
    /// asset; files that are not there or of the wrong kind, other assets and other discs are refused; `null`
    /// forgets them.
    #[test]
    fn transcoded_files_are_located_and_forgotten() -> Result<(), Box<dyn std::error::Error>> {
        let dir =
            std::env::temp_dir().join(format!("disccraft-engine-locate-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let (source, video, sound, picture) = (
            dir.join("source.mp4"),
            dir.join("movie.m2v"),
            dir.join("movie.ac3"),
            dir.join("still.png"),
        );
        for file in [&source, &video, &sound, &picture] {
            std::fs::write(file, b"x")?;
        }
        let mut session = Session::default();
        let asset = session
            .execute("asset.import", &json!({ "path": source }))?
            .get("asset")
            .and_then(Value::as_u64)
            .ok_or("asset")?;
        let still = session
            .execute("asset.import", &json!({ "path": picture }))?
            .get("asset")
            .and_then(Value::as_u64)
            .ok_or("asset")?;
        let locate = |session: &mut Session, params: Value| {
            session.execute("asset.locateTranscoded", &params)
        };
        for bad in [
            json!({ "asset": asset, "disc": "dvd", "video": dir.join("none.m2v") }),
            json!({ "asset": asset, "disc": "dvd", "video": sound }),
            json!({ "asset": asset, "disc": "dvd", "video": video, "audio": picture }),
            json!({ "asset": asset, "disc": "hd-dvd", "video": video }),
            json!({ "asset": still, "disc": "dvd", "video": video }),
            json!({ "asset": asset, "disc": "dvd" }),
        ] {
            assert!(locate(&mut session, bad.clone()).is_err(), "{bad}");
        }
        locate(
            &mut session,
            json!({ "asset": asset, "disc": "dvd", "video": video, "audio": sound }),
        )?;
        let listed = session.execute("asset.list", &json!({}))?;
        let entry = listed
            .as_array()
            .and_then(|list| {
                list.iter()
                    .find(|entry| entry.get("id").and_then(Value::as_u64) == Some(asset))
            })
            .ok_or("listed")?;
        assert!(
            entry
                .pointer("/locatedDvd/video")
                .and_then(Value::as_str)
                .is_some_and(|path| path.ends_with("movie.m2v"))
        );
        assert!(entry.get("locatedBluray").is_none());
        locate(
            &mut session,
            json!({ "asset": asset, "disc": "dvd", "video": null }),
        )?;
        let id = AssetId(asset);
        assert!(
            session
                .project
                .assets
                .get(&id)
                .is_some_and(|asset| asset.located_dvd.is_none())
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// An asset can be pointed at another file of the same kind; the name stays and it is online again.
    /// M6.273: the frame of a video asset is saved as a PNG in square pixels (a 720 x 480 4:3 picture as
    /// 640 x 480); what is not a video, and a place that cannot be written, are refused.
    #[test]
    fn a_frame_is_saved_as_a_picture_file() {
        let video = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/fixtures/ntsc.m2v");
        if !video.is_file() {
            eprintln!("ntsc.m2v fixture not found; skipping");
            return;
        }
        let dir = std::env::temp_dir().join(format!("disccraft-save-frame-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(std::fs::create_dir_all(&dir).is_ok());
        let mut session = crate::Session::new();
        let asset = session
            .execute(
                "asset.import",
                &serde_json::json!({"path": video.to_string_lossy()}),
            )
            .ok()
            .and_then(|value| value.get("asset").and_then(serde_json::Value::as_u64));
        let out = dir.join("frame.png");
        let saved = session.execute(
            "asset.saveFrame",
            &serde_json::json!({"asset": asset, "seconds": 1.0, "path": out.to_string_lossy()}),
        );
        assert!(saved.is_ok(), "{saved:?}");
        let size = saved.ok().map(|value| {
            (
                value.get("width").and_then(serde_json::Value::as_u64),
                value.get("height").and_then(serde_json::Value::as_u64),
            )
        });
        assert_eq!(size, Some((Some(640), Some(480))));
        // A PNG of that size: its signature and the size in its header.
        let bytes = std::fs::read(&out).unwrap_or_default();
        assert_eq!(bytes.get(..8), Some(&[137, 80, 78, 71, 13, 10, 26, 10][..]));
        assert_eq!(bytes.get(16..24), Some(&[0, 0, 2, 128, 0, 0, 1, 224][..]));
        assert!(
            session
                .execute(
                    "asset.saveFrame",
                    &serde_json::json!({"asset": asset, "seconds": 1.0, "path": dir.join("no/such/dir/f.png").to_string_lossy()}),
                )
                .is_err()
        );
        assert!(
            session
                .execute(
                    "asset.saveFrame",
                    &serde_json::json!({"asset": 9999, "seconds": 1.0, "path": out.to_string_lossy()}),
                )
                .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

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
    /// M6.253: a picture larger than a thumbnail is made when an edge is asked for (a slide shown in the preview).
    #[test]
    fn a_thumbnail_is_made_at_the_edge_asked_for() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-thumb-edge-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let drawing = dir.join("wide.svg");
        std::fs::write(
            &drawing,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200"><rect width="400" height="200" fill="#c04020"/></svg>"##,
        )?;
        let mut session = Session::default();
        let imported =
            session.execute("asset.import", &json!({"path": drawing.to_string_lossy()}))?;
        let id = imported
            .get("asset")
            .and_then(Value::as_u64)
            .ok_or("no asset id")?;
        let size = |session: &mut Session, params: Value| -> Result<(u64, u64, usize), String> {
            let made = session.execute("asset.thumbnail", &params)?;
            Ok((
                made.get("width").and_then(Value::as_u64).unwrap_or(0),
                made.get("height").and_then(Value::as_u64).unwrap_or(0),
                made.get("rgb")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len),
            ))
        };
        // A thumbnail: 128 on its long edge.
        assert_eq!(
            size(&mut session, json!({"asset": id}))?,
            (128, 64, 128 * 64 * 3)
        );
        // For the preview: 720.
        assert_eq!(
            size(&mut session, json!({"asset": id, "edge": 720}))?,
            (720, 360, 720 * 360 * 3)
        );
        // Outside what is made: the nearest.
        assert_eq!(size(&mut session, json!({"asset": id, "edge": 4}))?.0, 16);
        assert_eq!(
            size(&mut session, json!({"asset": id, "edge": 99_999}))?.0,
            1920
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// M6.325: whether a video goes on a title of a Blu-ray format as it is, asked of any format (SD too); an unknown
    /// format or asset is refused.
    #[test]
    fn a_video_is_asked_whether_it_fits_a_format() -> Result<(), Box<dyn std::error::Error>> {
        let mut session = Session::default();
        let video = session
            .project
            .add_asset(std::path::Path::new("missing-clip.mp4"))?;
        // Its file is not there: it fits none.
        for format in ["sd", "720p59.94"] {
            let fits = session.execute(
                "asset.fitsBluray",
                &json!({"asset": video.0, "video": format}),
            )?;
            assert_eq!(fits.get("fits"), Some(&Value::Bool(false)), "{format}");
        }
        for bad in [
            json!({"asset": video.0, "video": "1080p30"}),
            json!({"asset": 99, "video": "sd"}),
            json!({"asset": video.0}),
        ] {
            assert!(session.execute("asset.fitsBluray", &bad).is_err(), "{bad}");
        }
        Ok(())
    }

    /// M6.326: a preset's quality (Encore's: "Low quality" 2, "Long Form" MPEG-2 3, the others 5) is kept with
    /// the DVD setting while it has a rate of its own (kept when left out, the best for automatic) and with the
    /// Blu-ray one (the best when left out); outside 1 to 5 it is refused.
    #[test]
    fn a_transcode_setting_has_its_quality() -> Result<(), Box<dyn std::error::Error>> {
        use disccraft_project::BEST_QUALITY;
        let mut session = Session::default();
        let video = session
            .project
            .add_asset(std::path::Path::new("clip.mp4"))?;
        let dvd = |session: &Session| {
            session
                .project
                .assets
                .get(&video)
                .map(|asset| asset.transcode_quality)
        };
        let bluray = |session: &Session| {
            session
                .project
                .assets
                .get(&video)
                .and_then(|asset| asset.bluray_transcode.as_ref())
                .map(|setting| setting.quality)
        };
        assert_eq!(dvd(&session), Some(BEST_QUALITY));
        let _ = session.execute(
            "asset.setTranscode",
            &json!({"asset": video.0, "kbps": 4000, "mode": "cbr", "quality": 2}),
        )?;
        assert_eq!(dvd(&session), Some(2));
        let _ = session.execute(
            "asset.setTranscode",
            &json!({"asset": video.0, "kbps": 5000}),
        )?;
        assert_eq!(dvd(&session), Some(2));
        let _ = session.execute(
            "asset.setTranscode",
            &json!({"asset": video.0, "kbps": null}),
        )?;
        assert_eq!(dvd(&session), Some(BEST_QUALITY));
        let _ = session.execute(
            "asset.setBlurayTranscode",
            &json!({"asset": video.0, "kbps": 9000, "maxKbps": 11000, "quality": 3}),
        )?;
        assert_eq!(bluray(&session), Some(3));
        let _ = session.execute(
            "asset.setBlurayTranscode",
            &json!({"asset": video.0, "kbps": 25000, "maxKbps": 30000}),
        )?;
        assert_eq!(bluray(&session), Some(BEST_QUALITY));
        // M6.328: and how its rate is kept (one-pass VBR when left out).
        let mode = |session: &Session| {
            session
                .project
                .assets
                .get(&video)
                .and_then(|asset| asset.bluray_transcode.as_ref())
                .map(|setting| setting.mode)
        };
        assert_eq!(mode(&session), Some(disccraft_project::TranscodeMode::Vbr));
        let _ = session.execute(
            "asset.setBlurayTranscode",
            &json!({"asset": video.0, "kbps": 20000, "maxKbps": 30000, "mode": "vbr2Pass"}),
        )?;
        assert_eq!(
            mode(&session),
            Some(disccraft_project::TranscodeMode::Vbr2Pass)
        );
        assert!(
            session
                .execute(
                    "asset.setBlurayTranscode",
                    &json!({"asset": video.0, "kbps": 20000, "mode": "twice"}),
                )
                .is_err()
        );
        for (command, bad) in [
            ("asset.setTranscode", json!(0)),
            ("asset.setTranscode", json!(6)),
            ("asset.setBlurayTranscode", json!("high")),
        ] {
            let params = json!({"asset": video.0, "kbps": 4000, "quality": bad});
            assert!(
                session.execute(command, &params).is_err(),
                "{command} {params}"
            );
        }
        Ok(())
    }

    /// M6.327: Encore's rendering checkboxes are kept per video (each left as it was when left out); a still or a
    /// flag that is not true or false is refused.
    #[test]
    fn a_video_keeps_its_rendering() -> Result<(), Box<dyn std::error::Error>> {
        let mut session = Session::default();
        let video = session
            .project
            .add_asset(std::path::Path::new("clip.mp4"))?;
        let still = session
            .project
            .add_asset(std::path::Path::new("cover.png"))?;
        let flags = |session: &Session| {
            session
                .project
                .assets
                .get(&video)
                .map(|asset| (asset.max_render_quality, asset.frame_blend))
        };
        assert_eq!(flags(&session), Some((false, false)));
        let _ = session.execute(
            "asset.setRendering",
            &json!({"asset": video.0, "maxRenderQuality": true}),
        )?;
        assert_eq!(flags(&session), Some((true, false)));
        let _ = session.execute(
            "asset.setRendering",
            &json!({"asset": video.0, "frameBlend": true}),
        )?;
        assert_eq!(flags(&session), Some((true, true)));
        let _ = session.execute(
            "asset.setRendering",
            &json!({"asset": video.0, "maxRenderQuality": false, "frameBlend": false}),
        )?;
        assert_eq!(flags(&session), Some((false, false)));
        for bad in [
            json!({"asset": still.0, "frameBlend": true}),
            json!({"asset": video.0, "frameBlend": "yes"}),
            json!({"asset": 99, "frameBlend": true}),
        ] {
            assert!(
                session.execute("asset.setRendering", &bad).is_err(),
                "{bad}"
            );
        }
        Ok(())
    }

    /// M6.300: Transcode Settings keeps how the rate is kept beside it (VBR 1 pass, VBR 2 pass, CBR): set with the
    /// rate or alone, kept when left out, undone; another name is refused.
    /// M10.39: a codec of its own (kept when left out), the project's with null or with the project's format;
    /// automatic and unknown ones refused. (Part of [`a_transcode_setting_has_its_encoding`], for asset 5.)
    fn codec_of_its_own(session: &mut Session) -> Result<(), Box<dyn std::error::Error>> {
        let format = |session: &Session| {
            session
                .project
                .assets
                .get(&AssetId(5))
                .and_then(|asset| asset.bluray_video)
        };
        let codec = |session: &Session| {
            session
                .project
                .assets
                .get(&AssetId(5))
                .and_then(|asset| asset.bluray_codec)
        };
        let _ = session.execute(
            "asset.setBlurayTranscode",
            &json!({"asset": 5, "kbps": null, "video": "720p59.94", "codec": "mpeg2"}),
        )?;
        assert_eq!(codec(session), Some(disccraft_project::BdCodec::Mpeg2));
        let _ = session.execute(
            "asset.setBlurayTranscode",
            &json!({"asset": 5, "kbps": 20000}),
        )?;
        assert_eq!(codec(session), Some(disccraft_project::BdCodec::Mpeg2));
        for bad in [json!("auto"), json!("vc1"), json!(2)] {
            let params = json!({"asset": 5, "kbps": null, "codec": bad});
            assert!(
                session
                    .execute("asset.setBlurayTranscode", &params)
                    .is_err(),
                "{params}"
            );
        }
        let _ = session.execute(
            "asset.setBlurayTranscode",
            &json!({"asset": 5, "kbps": null, "codec": null}),
        )?;
        assert_eq!(codec(session), None);
        assert_eq!(format(session), Some(disccraft_project::BdVideo::Hd720p59));
        let _ = session.execute(
            "asset.setBlurayTranscode",
            &json!({"asset": 5, "kbps": null, "codec": "h264"}),
        )?;
        let _ = session.execute(
            "asset.setBlurayTranscode",
            &json!({"asset": 5, "kbps": null, "video": null}),
        )?;
        assert_eq!(format(session), None);
        assert_eq!(codec(session), None);
        Ok(())
    }

    #[test]
    fn a_transcode_setting_has_its_encoding() -> Result<(), Box<dyn std::error::Error>> {
        use disccraft_project::TranscodeMode;
        let mut session = Session::default();
        let video = Asset {
            id: AssetId(5),
            name: "clip".to_string(),
            path: "clip.mp4".to_string(),
            kind: AssetKind::Video,
            offline: false,
            cues: None,
            composition: None,
            description: String::new(),
            transcode_kbps: None,
            pixel_aspect: None,
            located_dvd: None,
            located_bluray: None,
            ignore_alpha: false,
            transcode_mode: Default::default(),
            transcode_audio: Default::default(),
            transcode_preset: None,
            bluray_transcode: None,
            transcode_film: false,
            bluray_video: None,
            bluray_codec: None,
            transcode_quality: disccraft_project::BEST_QUALITY,
            max_render_quality: false,
            frame_blend: false,
        };
        let _ = session.project.assets.insert(AssetId(5), video);
        let setting = |session: &Session| {
            session
                .project
                .assets
                .get(&AssetId(5))
                .map(|asset| (asset.transcode_kbps, asset.transcode_mode))
        };
        assert_eq!(setting(&session), Some((None, TranscodeMode::Vbr)));
        let _ = session.execute(
            "asset.setTranscode",
            &json!({"asset": 5, "kbps": 7000, "mode": "vbr2Pass"}),
        )?;
        assert_eq!(
            setting(&session),
            Some((Some(7000), TranscodeMode::Vbr2Pass))
        );
        // Left out: kept.
        let _ = session.execute("asset.setTranscode", &json!({"asset": 5, "kbps": 8000}))?;
        assert_eq!(
            setting(&session),
            Some((Some(8000), TranscodeMode::Vbr2Pass))
        );
        let _ = session.execute(
            "asset.setTranscode",
            &json!({"asset": 5, "kbps": 8000, "mode": "cbr"}),
        )?;
        assert_eq!(setting(&session), Some((Some(8000), TranscodeMode::Cbr)));
        let _ = session.execute("edit.undo", &json!({}))?;
        assert_eq!(
            setting(&session),
            Some((Some(8000), TranscodeMode::Vbr2Pass))
        );
        // M6.307: the DVD preset's name is kept with a rate, forgotten without one.
        let _ = session.execute(
            "asset.setTranscode",
            &json!({"asset": 5, "kbps": 7000, "mode": "vbr2Pass", "preset": "NTSC DV High quality 7Mb VBR 2 Pass"}),
        )?;
        assert_eq!(
            session
                .project
                .assets
                .get(&AssetId(5))
                .and_then(|asset| asset.transcode_preset.clone()),
            Some("NTSC DV High quality 7Mb VBR 2 Pass".to_owned())
        );
        let _ = session.execute(
            "asset.setTranscode",
            &json!({"asset": 5, "kbps": null, "preset": "x"}),
        )?;
        assert_eq!(
            session
                .project
                .assets
                .get(&AssetId(5))
                .and_then(|asset| asset.transcode_preset.clone()),
            None
        );
        // M6.314: film (a 23.976 preset) with a rate of its own; kept when left out, gone without a rate.
        let film = |session: &Session| {
            session
                .project
                .assets
                .get(&AssetId(5))
                .map(|asset| asset.transcode_film)
        };
        let _ = session.execute(
            "asset.setTranscode",
            &json!({"asset": 5, "kbps": 7000, "film": true, "preset": "NTSC DV 23.976 High quality 7Mb VBR 2 Pass"}),
        )?;
        assert_eq!(film(&session), Some(true));
        let _ = session.execute("asset.setTranscode", &json!({"asset": 5, "kbps": 4000}))?;
        assert_eq!(film(&session), Some(true));
        assert!(
            session
                .execute(
                    "asset.setTranscode",
                    &json!({"asset": 5, "kbps": 4000, "film": 1})
                )
                .is_err()
        );
        let _ = session.execute(
            "asset.setTranscode",
            &json!({"asset": 5, "kbps": null, "film": true}),
        )?;
        assert_eq!(film(&session), Some(false));
        // The Blu-ray setting: its own target, peak and preset; automatic with null.
        let _ = session.execute(
            "asset.setBlurayTranscode",
            &json!({"asset": 5, "kbps": 25000, "maxKbps": 30000, "preset": "1920x1080i 29.97 High Quality MPEG-2"}),
        )?;
        assert_eq!(
            session
                .project
                .assets
                .get(&AssetId(5))
                .and_then(|asset| asset.bluray_transcode.clone()),
            Some(disccraft_project::BlurayTranscode {
                kbps: 25000,
                max_kbps: 30000,
                preset: "1920x1080i 29.97 High Quality MPEG-2".to_owned(),
                quality: disccraft_project::BEST_QUALITY,
                mode: disccraft_project::TranscodeMode::Vbr,
            })
        );
        for bad in [
            json!({"asset": 5, "kbps": 50}),
            json!({"asset": 5, "kbps": 9000, "maxKbps": 90000}),
        ] {
            assert!(
                session.execute("asset.setBlurayTranscode", &bad).is_err(),
                "{bad}"
            );
        }
        let _ = session.execute(
            "asset.setBlurayTranscode",
            &json!({"asset": 5, "kbps": null}),
        )?;
        assert_eq!(
            session
                .project
                .assets
                .get(&AssetId(5))
                .and_then(|asset| asset.bluray_transcode.clone()),
            None
        );
        // M6.325: an HD format of its own (kept when left out), the project's with null; SD and unknown ones
        // refused.
        let format = |session: &Session| {
            session
                .project
                .assets
                .get(&AssetId(5))
                .and_then(|asset| asset.bluray_video)
        };
        let _ = session.execute(
            "asset.setBlurayTranscode",
            &json!({"asset": 5, "kbps": null, "video": "720p59.94"}),
        )?;
        assert_eq!(format(&session), Some(disccraft_project::BdVideo::Hd720p59));
        let _ = session.execute(
            "asset.setBlurayTranscode",
            &json!({"asset": 5, "kbps": 8000}),
        )?;
        assert_eq!(format(&session), Some(disccraft_project::BdVideo::Hd720p59));
        for bad in [json!("sd"), json!("1080p30"), json!(7)] {
            let params = json!({"asset": 5, "kbps": null, "video": bad});
            assert!(
                session
                    .execute("asset.setBlurayTranscode", &params)
                    .is_err(),
                "{params}"
            );
        }
        codec_of_its_own(&mut session)?;
        let _ = session.execute("asset.setTranscode", &json!({"asset": 5, "kbps": 8000}))?;
        // M6.306: the sound's format beside them.
        let _ = session.execute(
            "asset.setTranscode",
            &json!({"asset": 5, "kbps": 8000, "audio": "pcm"}),
        )?;
        assert_eq!(
            session
                .project
                .assets
                .get(&AssetId(5))
                .map(|asset| asset.transcode_audio),
            Some(disccraft_project::TranscodeAudio::Pcm)
        );
        assert!(
            session
                .execute(
                    "asset.setTranscode",
                    &json!({"asset": 5, "kbps": 8000, "audio": "mp3"})
                )
                .is_err()
        );
        assert!(
            session
                .execute(
                    "asset.setTranscode",
                    &json!({"asset": 5, "kbps": null, "mode": "fast"}),
                )
                .is_err()
        );
        Ok(())
    }

    /// M6.294: Interpret Footage ▸ Ignore Alpha Channel is a picture's: the preview's slide is then its colours
    /// as they are, otherwise laid over black (as the disc shows it); a video asset has none.
    #[test]
    fn a_pictures_alpha_channel_can_be_ignored() -> Result<(), Box<dyn std::error::Error>> {
        let dir =
            std::env::temp_dir().join(format!("disccraft-ignore-alpha-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("half.png");
        let file = std::io::BufWriter::new(std::fs::File::create(&path)?);
        let mut encoder = png::Encoder::new(file, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()?
            .write_image_data(&[200, 100, 50, 128])?;
        let mut session = Session::default();
        let id = session
            .execute("asset.import", &json!({"path": path.to_string_lossy()}))?
            .get("asset")
            .and_then(Value::as_u64)
            .ok_or("no asset id")?;
        let first = |session: &mut Session| -> Result<Vec<u64>, String> {
            let made = session.execute("asset.thumbnail", &json!({"asset": id, "edge": 720}))?;
            Ok(made
                .get("rgb")
                .and_then(Value::as_array)
                .map(|rgb| rgb.iter().take(3).filter_map(Value::as_u64).collect())
                .unwrap_or_default())
        };
        let ignored = |session: &mut Session| -> Result<Option<bool>, String> {
            let found = session.execute("asset.pixelAspect", &json!({"asset": id}))?;
            Ok(found.get("ignoreAlpha").and_then(Value::as_bool))
        };
        assert_eq!(ignored(&mut session)?, Some(false));
        assert_eq!(first(&mut session)?, vec![100, 50, 25]);
        let _ = session.execute(
            "asset.setIgnoreAlpha",
            &json!({"asset": id, "ignore": true}),
        )?;
        assert_eq!(ignored(&mut session)?, Some(true));
        assert_eq!(first(&mut session)?, vec![200, 100, 50]);
        // Undone: as it was.
        let _ = session.execute("edit.undo", &json!({}))?;
        assert_eq!(ignored(&mut session)?, Some(false));
        // Only a picture.
        let video = Asset {
            id: AssetId(99),
            name: "clip".to_string(),
            path: "clip.mp4".to_string(),
            kind: AssetKind::Video,
            offline: false,
            cues: None,
            composition: None,
            description: String::new(),
            transcode_kbps: None,
            pixel_aspect: None,
            located_dvd: None,
            located_bluray: None,
            ignore_alpha: false,
            transcode_mode: Default::default(),
            transcode_audio: Default::default(),
            transcode_preset: None,
            bluray_transcode: None,
            transcode_film: false,
            bluray_video: None,
            bluray_codec: None,
            transcode_quality: disccraft_project::BEST_QUALITY,
            max_render_quality: false,
            frame_blend: false,
        };
        let _ = session.project.assets.insert(AssetId(99), video);
        assert!(
            session
                .execute(
                    "asset.setIgnoreAlpha",
                    &json!({"asset": 99, "ignore": true})
                )
                .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

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
            transcode_kbps: None,
            pixel_aspect: None,
            located_dvd: None,
            located_bluray: None,
            ignore_alpha: false,
            transcode_mode: Default::default(),
            transcode_audio: Default::default(),
            transcode_preset: None,
            bluray_transcode: None,
            transcode_film: false,
            bluray_video: None,
            bluray_codec: None,
            transcode_quality: disccraft_project::BEST_QUALITY,
            max_render_quality: false,
            frame_blend: false,
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

    /// An MKV of MPEG-4 Part 2 video (DivX / Xvid, which craft-video does not decode) and AC-3, made with ffmpeg
    /// into the fixtures (as the player's tests make it); `None` without ffmpeg.
    fn mpeg4_part2_fixture() -> Option<std::path::PathBuf> {
        let fixtures =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
        let video = fixtures.join("mpeg4_part2_ac3.mkv");
        if video.is_file() {
            return Some(video);
        }
        let ffmpeg = std::env::var_os("DISCCRAFT_FFMPEG").unwrap_or_else(|| "ffmpeg".into());
        std::fs::create_dir_all(&fixtures).ok()?;
        let part = fixtures.join(format!("mpeg4_part2_ac3.a{}.mkv", std::process::id()));
        let made = std::process::Command::new(ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=640x480:rate=25")
            .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
            .args([
                "-t", "2", "-c:v", "mpeg4", "-q:v", "4", "-c:a", "ac3", "-b:a", "192k",
            ])
            .arg(&part)
            .status()
            .ok()?;
        if !made.success() || std::fs::rename(&part, &video).is_err() {
            let _ = std::fs::remove_file(&part);
            return None;
        }
        Some(video)
    }

    /// M6.245: a video whose picture does not decode (MPEG-4 Part 2; interlaced H.264 until craft-video decoded
    /// it, M12.25) is an error of the project check on a disc that would have to convert it, once the file is open;
    /// a Blu-ray cannot carry it as it is either.
    #[test]
    fn a_video_that_cannot_be_converted_is_a_check_error() -> Result<(), Box<dyn std::error::Error>>
    {
        let Some(fixture) = mpeg4_part2_fixture() else {
            eprintln!("no ffmpeg for the MPEG-4 Part 2 fixture; skipping");
            return Ok(());
        };
        let dir =
            std::env::temp_dir().join(format!("disccraft-undecodable-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let video = dir.join("divx.mkv");
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
        // Asked until the file is open (its length is known at once from its header; the thumbnail's question
        // opens it). The check is asked each time, not once after the wait: the tests that run beside this one
        // open files too and can push it out of the few kept open (M6.358), and then the check cannot tell.
        let start = std::time::Instant::now();
        loop {
            let _ = session.execute("asset.pending", &json!({}))?;
            let _ = session.execute("asset.thumbnail", &json!({"asset": id, "wait": false}));
            if undecodable(&mut session)? {
                break;
            }
            assert!(
                start.elapsed() < std::time::Duration::from_secs(60),
                "a DVD would have to convert it"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // A Blu-ray would have to convert it too.
        session.execute("disc.setKind", &json!({"kind": "bluRay"}))?;
        session.execute("disc.setBlurayVideo", &json!({"video": "1080i59.94"}))?;
        assert!(undecodable(&mut session)?);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// M12.25: interlaced H.264 (a camcorder's 1080i, made by x264 as MBAFF) is no check error on a DVD any more:
    /// craft-video decodes it (storytold/craft-libs PRs #1992 and #2017, through the mOqOm fork), so its thumbnail
    /// is made and a DVD converts it.
    #[test]
    fn interlaced_h264_can_be_converted() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/fixtures/h264_1080i.mkv");
        if !fixture.is_file() {
            eprintln!("h264_1080i.mkv fixture not found; skipping");
            return Ok(());
        }
        let mut session = Session::new();
        let id = session
            .execute("asset.import", &json!({"path": fixture.to_string_lossy()}))?
            .get("asset")
            .and_then(Value::as_u64)
            .ok_or("no asset")?;
        let _ = session.execute("timeline.create", &json!({"asset": id}))?;
        let thumbnail = session.execute("asset.thumbnail", &json!({"asset": id}))?;
        // A picture: some of its samples are not black.
        let lit = thumbnail
            .get("rgb")
            .and_then(Value::as_array)
            .map_or(0, |rgb| {
                rgb.iter()
                    .filter(|v| v.as_u64().is_some_and(|v| v > 40))
                    .count()
            });
        assert!(lit > 1000, "{lit} lit samples");
        let check = session.execute("project.check", &json!({}))?;
        let problems = check
            .get("problems")
            .and_then(Value::as_array)
            .ok_or("no problems")?;
        assert!(
            !problems
                .iter()
                .any(|problem| problem.get("code") == Some(&json!("undecodableVideo"))),
            "{check}"
        );
        Ok(())
    }

    /// M12.13: asked without waiting, a video that is not open yet is opened on its own thread and answered
    /// "pending"; once `asset.pending` lists nothing (neither pending nor opening), the same question is answered
    /// at once.
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

        // Once neither list names it ("opening": its facts are known, its pictures not yet), all three are
        // answered without a wait. They are asked again when one is still pending: the tests that run beside this
        // one open files too and can push it out of the few kept open (M6.358), and then the lists name it again.
        let start = std::time::Instant::now();
        let (probe, thumbnail, shown) = loop {
            let lists = session.execute("asset.pending", &json!({}))?;
            if lists == json!({"pending": [], "opening": []}) {
                let probe = session.execute("asset.probe", &json!({"asset": id, "wait": false}))?;
                let thumbnail =
                    session.execute("asset.thumbnail", &json!({"asset": id, "wait": false}))?;
                let shown = session.execute("asset.frame", &frame)?;
                if [&probe, &thumbnail, &shown]
                    .iter()
                    .all(|answer| answer.get("pending").is_none())
                {
                    break (probe, thumbnail, shown);
                }
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(60));
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert_eq!(
            probe.get("width").and_then(Value::as_u64),
            Some(720),
            "{probe}"
        );
        assert!(thumbnail.get("rgb").is_some(), "{thumbnail}");
        assert!(shown.get("rgb").is_some(), "{shown}");

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
