//! Craft アプリとの連携（Encore の Photoshop / Audition / After Effects / Premiere との連携にあたる）。
//!
//! - 編集 ▸ メニューを PhotoCraft で編集（Shift+Ctrl+M）: メニューをレイヤー付きの PSD（背景、テキスト、ボタン
//!   ごとの `(+)` グループと `(=1)` ハイライト）に書き出して PhotoCraft で開き、保存されたら `menu.applyPsd` で
//!   反映する。メニューの背景が既にその PSD なら、PSD をそのまま開く。
//! - 編集 ▸ オリジナルを編集（Ctrl+E）: アセットを作ったアプリで開く（音声は SoundCraft、画像は PhotoCraft、
//!   ビデオは FilmCraft か EffectCraft）。保存されたら読み直す。
//! - メニュー ▸ EffectCraft コンポジションを作成…: メニューの PSD を EffectCraft のコンポジションにし、レンダー
//!   キューに `<メニュー> モーション.mp4` を入れて開く。レンダーされたら `menu.applyMotion` でモーション
//!   メニューの背景にする。
//!
//! アプリが見つからないときは、ファイルをシステムの既定のアプリで開く。

use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf};
use disccraft_project::{AssetId, AssetKind, Menu, MenuBackground, MenuId, VideoStandard};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// One of the Craft apps DiscCraft works with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CraftApp {
    Photo,
    Sound,
    Effect,
    Film,
}

impl CraftApp {
    /// The app's name.
    pub(crate) fn name(self) -> &'static str {
        match self {
            CraftApp::Photo => "PhotoCraft",
            CraftApp::Sound => "SoundCraft",
            CraftApp::Effect => "EffectCraft",
            CraftApp::Film => "FilmCraft",
        }
    }

    /// The program's file name without extension.
    fn program(self) -> &'static str {
        match self {
            CraftApp::Photo => "photocraft",
            CraftApp::Sound => "soundcraft",
            CraftApp::Effect => "effectcraft",
            CraftApp::Film => "filmcraft",
        }
    }

    /// The environment variable that names the program (to use one installed elsewhere).
    fn variable(self) -> &'static str {
        match self {
            CraftApp::Photo => "DISCCRAFT_PHOTOCRAFT",
            CraftApp::Sound => "DISCCRAFT_SOUNDCRAFT",
            CraftApp::Effect => "DISCCRAFT_EFFECTCRAFT",
            CraftApp::Film => "DISCCRAFT_FILMCRAFT",
        }
    }

    /// The apps that edit an asset of `kind`, first choice first (Encore's Edit Original opens the app that made it).
    pub(crate) fn for_asset(kind: AssetKind) -> &'static [CraftApp] {
        match kind {
            AssetKind::Audio => &[CraftApp::Sound],
            AssetKind::Image => &[CraftApp::Photo],
            AssetKind::Video => &[CraftApp::Film, CraftApp::Effect],
            AssetKind::Subtitle | AssetKind::Other => &[],
        }
    }
}

/// What a watched file is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditTarget {
    /// A menu's PSD (Edit Menu in PhotoCraft).
    Menu(MenuId),
    /// An asset's own file (Edit Original).
    Asset(AssetId),
    /// The movie EffectCraft renders for a menu (a motion menu).
    Motion(MenuId),
}

/// A file being edited in another app, watched for saves.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ExternalEdit {
    pub(crate) target: EditTarget,
    pub(crate) path: PathBuf,
    /// The file's time when it was last applied (or written).
    pub(crate) applied: Option<SystemTime>,
    /// A new time and size seen once; applied when the next check sees the same (the editor has finished writing).
    pub(crate) pending: Option<(SystemTime, u64)>,
}

/// A Craft app's program: its environment variable, the usual install folders (`ArtCraft\<App>`), or the PATH.
pub(crate) fn find_app(app: CraftApp) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(app.variable()).map(PathBuf::from)
        && path.is_file()
    {
        return Some(path);
    }
    find_program(app, app.program())
}

/// A Craft app's command-line program (`<app>-cli`), next to the app.
pub(crate) fn find_cli(app: CraftApp) -> Option<PathBuf> {
    let cli = format!("{}-cli", app.program());
    if let Some(folder) = find_app(app).as_deref().and_then(Path::parent) {
        let path = folder.join(format!("{cli}{}", std::env::consts::EXE_SUFFIX));
        if path.is_file() {
            return Some(path);
        }
    }
    find_program(app, &cli)
}

fn find_program(app: CraftApp, program: &str) -> Option<PathBuf> {
    let exe = format!("{program}{}", std::env::consts::EXE_SUFFIX);
    let mut candidates = Vec::new();
    for var in ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var).map(PathBuf::from) {
            candidates.push(base.join("ArtCraft").join(app.name()).join(&exe));
            candidates.push(
                base.join("Programs")
                    .join("ArtCraft")
                    .join(app.name())
                    .join(&exe),
            );
            candidates.push(base.join(app.name()).join(&exe));
        }
    }
    candidates.push(
        PathBuf::from("/Applications")
            .join(format!("{}.app", app.name()))
            .join("Contents")
            .join("MacOS")
            .join(&exe),
    );
    if let Some(found) = candidates.into_iter().find(|path| path.is_file()) {
        return Some(found);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(&exe))
            .find(|path| path.is_file())
    })
}

/// A file name made from a menu name (characters a file name cannot hold become "_").
pub(crate) fn file_name_for(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if cleaned.is_empty() {
        "menu".to_owned()
    } else {
        cleaned.to_owned()
    }
}

/// Whether `path` names a PSD / PSB file.
fn is_psd_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("psd") || ext.eq_ignore_ascii_case("psb"))
}

/// The time and size of a file, if it exists.
fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

/// `<folder>/<base>.<ext>`, or `<base> 2.<ext>`, `<base> 3.<ext>`, … when that is taken.
fn unique_path(folder: &Path, base: &str, ext: &str) -> PathBuf {
    let mut path = folder.join(format!("{base}.{ext}"));
    let mut number = 2_u32;
    while path.exists() && number < 1000 {
        path = folder.join(format!("{base} {number}.{ext}"));
        number = number.saturating_add(1);
    }
    path
}

impl DiscCraftApp {
    /// 編集 ▸ メニューを PhotoCraft で編集.
    pub(crate) fn edit_menu_in_photocraft(&mut self, id: MenuId) {
        let Some(menu) = self.session.project.menu(id).cloned() else {
            return;
        };
        let path = match self.menu_psd(&menu) {
            Some(path) => path,
            None => match self.write_menu_psd(&menu) {
                Ok(path) => path,
                Err(error) => {
                    self.status = error;
                    return;
                }
            },
        };
        let launched = match find_app(CraftApp::Photo) {
            Some(exe) => std::process::Command::new(exe)
                .arg(&path)
                .spawn()
                .map(|_| tl!("Opened in PhotoCraft. Save there to update the menu.")),
            None => open_with_default_app(&path)
                .map(|()| tl!("PhotoCraft was not found; the PSD opened in its default app. Save there to update the menu.")),
        };
        match launched {
            Ok(message) => {
                self.watch(EditTarget::Menu(id), path);
                self.status = message.to_owned();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// The PSD a menu already follows (its background, after an earlier edit), to open as it is.
    fn menu_psd(&self, menu: &Menu) -> Option<PathBuf> {
        let MenuBackground::Image { asset } = &menu.background else {
            return None;
        };
        let path = PathBuf::from(&self.session.project.assets.get(asset)?.path);
        let plain =
            menu.texts.is_empty() && menu.buttons.iter().all(|button| button.label.is_empty());
        (is_psd_path(&path) && plain && path.is_file()).then_some(path)
    }

    /// Watches `path` for saves (one watch per target).
    fn watch(&mut self, target: EditTarget, path: PathBuf) {
        self.external_edits.retain(|edit| edit.target != target);
        self.external_edits.push(ExternalEdit {
            target,
            applied: stamp(&path).map(|(time, _)| time),
            path,
            pending: None,
        });
    }

    /// The folder for the files made for other apps: `<project>.menus` next to the project (a temporary folder
    /// before the project is saved).
    fn craft_folder(&self) -> Result<PathBuf, String> {
        let folder = match self.session.path.as_deref() {
            Some(project) => {
                let stem = project
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("project");
                project
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(format!("{stem}.menus"))
            }
            None => std::env::temp_dir().join("DiscCraft").join("menus"),
        };
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("{}: {error}", folder.display()))?;
        Ok(folder)
    }

    /// Writes the menu as a new layered PSD (in [`Self::craft_folder`]).
    fn write_menu_psd(&mut self, menu: &Menu) -> Result<PathBuf, String> {
        let folder = self.craft_folder()?;
        let path = unique_path(&folder, &file_name_for(&menu.name), "psd");
        self.export_menu_psd_to(menu, &path)?;
        Ok(path)
    }

    /// Writes the menu as a layered PSD at `path` (a motion menu over its video's first frame).
    pub(crate) fn export_menu_psd_to(&mut self, menu: &Menu, path: &Path) -> Result<(), String> {
        let picture = match &menu.background {
            MenuBackground::Video { asset, .. } => self.first_video_frame_pub(*asset),
            _ => None,
        };
        disccraft_menu::export_menu_psd(
            &self.session.project,
            menu,
            picture
                .as_ref()
                .map(|(rgb, width, height)| (rgb.as_slice(), *width, *height)),
            path,
        )
        .map_err(|error| error.to_string())
    }

    /// Looks at the files being edited outside (once a second) and takes in a saved one.
    pub(crate) fn poll_external_edits(&mut self, ctx: &egui::Context) {
        if self.external_edits.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        let now = ctx.input(|input| input.time);
        if now - self.external_check < 1.0 {
            return;
        }
        self.external_check = now;

        let mut ready = Vec::new();
        for edit in &mut self.external_edits {
            let Some((time, size)) = stamp(&edit.path) else {
                continue;
            };
            if Some(time) == edit.applied {
                edit.pending = None;
                continue;
            }
            if edit.pending == Some((time, size)) && size > 0 {
                edit.applied = Some(time);
                edit.pending = None;
                ready.push((edit.target, edit.path.clone()));
            } else {
                edit.pending = Some((time, size));
            }
        }
        for (target, path) in ready {
            match target {
                EditTarget::Menu(menu) => self.apply_menu_psd(menu, &path),
                EditTarget::Asset(asset) => self.reload_asset(asset),
                EditTarget::Motion(menu) => self.apply_menu_motion(menu, &path),
            }
        }
    }

    fn apply_menu_psd(&mut self, menu: MenuId, path: &Path) {
        let applied = self.run_command(
            "menu.applyPsd",
            json!({ "menu": menu.0, "path": path.to_string_lossy() }),
        );
        let Some(result) = applied else {
            return;
        };
        // The PSD changed on disk: draw the menu and its thumbnails again.
        let asset = result
            .get("asset")
            .and_then(serde_json::Value::as_u64)
            .map(AssetId);
        self.forget_pictures(asset);
        self.status = tl!("Updated the menu from PhotoCraft.").to_owned();
    }

    /// Forgets everything drawn or measured from an asset's file and from the menus, so they are read again.
    fn forget_pictures(&mut self, asset: Option<AssetId>) {
        self.menu_texture = None;
        self.menu_thumbnails.clear();
        if let Some(asset) = asset {
            let _ = self.thumbnails.remove(&asset.0);
            let _ = self.last_frames.remove(&asset.0);
            let _ = self.first_frames.remove(&asset.0);
            let _ = self.durations.remove(&asset.0);
        }
    }

    /// 編集 ▸ オリジナルを編集: opens an asset in the Craft app that edits it, and reads it again when it is saved.
    pub(crate) fn edit_original(&mut self, id: AssetId) {
        let Some(asset) = self.session.project.assets.get(&id).cloned() else {
            return;
        };
        let path = PathBuf::from(&asset.path);
        if !path.is_file() {
            self.status = tlf!("{name} is offline.", name = asset.name);
            return;
        }
        let app = CraftApp::for_asset(asset.kind)
            .iter()
            .copied()
            .find_map(|app| find_app(app).map(|exe| (app, exe)));
        let launched = match &app {
            Some((app, exe)) => std::process::Command::new(exe).arg(&path).spawn().map(|_| {
                tlf!(
                    "Opened {name} in {app}. Save there to update it here.",
                    name = asset.name,
                    app = app.name()
                )
            }),
            None => open_with_default_app(&path).map(|()| {
                tlf!(
                    "Opened {name} in its default app. Save there to update it here.",
                    name = asset.name
                )
            }),
        };
        match launched {
            Ok(message) => {
                self.watch(EditTarget::Asset(id), path);
                self.status = message;
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// An asset's file was saved in another app: read it again.
    fn reload_asset(&mut self, asset: AssetId) {
        self.forget_pictures(Some(asset));
        self.transcode_dirty = true;
        let name = self
            .session
            .project
            .assets
            .get(&asset)
            .map_or_else(String::new, |asset| asset.name.clone());
        self.status = tlf!("Updated {name}.", name = name);
    }

    /// メニュー ▸ EffectCraft コンポジションを作成…: makes an EffectCraft project whose composition is the menu (its
    /// PSD layers, at the disc's frame rate and pixel shape, 10 seconds) with a render of `<menu> モーション.mp4`
    /// queued, and opens it. When the movie is rendered it becomes the menu's background.
    pub(crate) fn create_effectcraft_composition(&mut self, id: MenuId) {
        let Some(menu) = self.session.project.menu(id).cloned() else {
            return;
        };
        let (Some(app), Some(cli)) = (find_app(CraftApp::Effect), find_cli(CraftApp::Effect))
        else {
            self.status = tl!("EffectCraft was not found.").to_owned();
            return;
        };
        let standard = self.session.project.settings.standard;
        let result = self.write_menu_psd(&menu).and_then(|psd| {
            let folder = psd.parent().map(Path::to_path_buf).unwrap_or_default();
            let base = file_name_for(&menu.name);
            let project = unique_path(&folder, &base, "ecproj");
            let movie = folder.join(format!("{base} モーション.mp4"));
            build_effectcraft_project(&cli, &psd, &movie, &project, standard)?;
            Ok((project, movie))
        });
        let (project, movie) = match result {
            Ok(paths) => paths,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        match std::process::Command::new(app).arg(&project).spawn() {
            Ok(_) => {
                self.watch(EditTarget::Motion(id), movie);
                self.status = tl!(
                    "Opened the menu in EffectCraft. Render it there; the movie becomes the menu's background."
                )
                .to_owned();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// EffectCraft rendered a menu's movie: make it the menu's motion background.
    fn apply_menu_motion(&mut self, menu: MenuId, path: &Path) {
        let applied = self.run_command(
            "menu.applyMotion",
            json!({ "menu": menu.0, "path": path.to_string_lossy() }),
        );
        if let Some(result) = applied {
            let asset = result
                .get("asset")
                .and_then(serde_json::Value::as_u64)
                .map(AssetId);
            self.forget_pictures(asset);
            self.status = tl!("Updated the menu's motion from EffectCraft.").to_owned();
        }
    }
}

/// The disc's frame rate and pixel aspect (4:3 DVD pixels) for an EffectCraft composition.
fn composition_shape(standard: VideoStandard) -> (f64, f64) {
    match standard {
        VideoStandard::Ntsc => (30000.0 / 1001.0, 10.0 / 11.0),
        VideoStandard::Pal => (25.0, 59.0 / 54.0),
    }
}

/// The steps for `effectcraft-cli run --args-file`: import the PSD as a composition, set its frame rate, pixel
/// aspect and length, and queue an H.264 render to `movie`.
fn effectcraft_steps(psd: &Path, movie: &Path, standard: VideoStandard) -> Vec<String> {
    let (rate, aspect) = composition_shape(standard);
    vec![
        "file.import".to_owned(),
        json!({ "paths": [psd.to_string_lossy()], "importAs": "composition" }).to_string(),
        "comp.settings".to_owned(),
        json!({ "frameRate": rate, "pixelAspect": aspect, "duration": 10 }).to_string(),
        "renderQueue.add".to_owned(),
        json!({ "format": "h264", "output": movie.to_string_lossy() }).to_string(),
    ]
}

/// Builds the EffectCraft project with its command-line program.
fn build_effectcraft_project(
    cli: &Path,
    psd: &Path,
    movie: &Path,
    project: &Path,
    standard: VideoStandard,
) -> Result<(), String> {
    let steps = project.with_extension("steps.txt");
    std::fs::write(&steps, effectcraft_steps(psd, movie, standard).join("\n"))
        .map_err(|error| format!("{}: {error}", steps.display()))?;
    let output = std::process::Command::new(cli)
        .arg("run")
        .arg("--args-file")
        .arg(&steps)
        .arg("--empty")
        .arg("--save-as")
        .arg(project)
        .output()
        .map_err(|error| format!("{}: {error}", cli.display()))?;
    let _ = std::fs::remove_file(&steps);
    if output.status.success() && project.is_file() {
        Ok(())
    } else {
        Err(format!(
            "EffectCraft: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// Opens a file in the system's default app for it.
fn open_with_default_app(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let child = std::process::Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(path)
        .spawn();
    #[cfg(target_os = "macos")]
    let child = std::process::Command::new("open").arg(path).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let child = std::process::Command::new("xdg-open").arg(path).spawn();
    child.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::{CraftApp, effectcraft_steps, file_name_for, is_psd_path, unique_path};
    use disccraft_project::{AssetKind, VideoStandard};
    use std::path::Path;

    #[test]
    fn menu_names_become_file_names() {
        assert_eq!(file_name_for("メイン"), "メイン");
        assert_eq!(file_name_for("a/b:c*?"), "a_b_c__");
        assert_eq!(file_name_for("  ..  "), "menu");
        assert_eq!(file_name_for(""), "menu");
    }

    #[test]
    fn psd_paths_are_recognised() {
        assert!(is_psd_path(Path::new("menu.PSD")));
        assert!(is_psd_path(Path::new("menu.psb")));
        assert!(!is_psd_path(Path::new("menu.png")));
    }

    #[test]
    fn each_kind_of_asset_has_its_craft_app() {
        assert_eq!(CraftApp::for_asset(AssetKind::Audio), [CraftApp::Sound]);
        assert_eq!(CraftApp::for_asset(AssetKind::Image), [CraftApp::Photo]);
        assert_eq!(
            CraftApp::for_asset(AssetKind::Video),
            [CraftApp::Film, CraftApp::Effect]
        );
        assert!(CraftApp::for_asset(AssetKind::Subtitle).is_empty());
    }

    #[test]
    fn the_effectcraft_steps_make_a_composition_and_queue_the_movie() {
        let steps = effectcraft_steps(Path::new("m.psd"), Path::new("m.mp4"), VideoStandard::Pal);
        assert_eq!(steps.len(), 6);
        assert_eq!(steps.first().map(String::as_str), Some("file.import"));
        assert!(
            steps
                .get(1)
                .is_some_and(|step| step.contains("composition"))
        );
        assert!(
            steps
                .get(3)
                .is_some_and(|step| step.contains("\"frameRate\":25"))
        );
        assert!(steps.get(5).is_some_and(|step| step.contains("m.mp4")));
    }

    #[test]
    fn unique_paths_do_not_overwrite() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-unique-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let first = unique_path(&dir, "menu", "psd");
        std::fs::write(&first, b"x")?;
        let second = unique_path(&dir, "menu", "psd");
        assert_ne!(first, second);
        assert!(second.ends_with("menu 2.psd"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
