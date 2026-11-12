use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, String>;

const LAYERS: &[(&str, u8)] = &[
    ("disccraft-mpeg2venc", 0),
    ("disccraft-ac3enc", 0),
    ("disccraft-psmux", 0),
    ("disccraft-spu", 0),
    ("disccraft-ifo", 0),
    ("disccraft-m2tsmux", 0),
    ("disccraft-bdmv", 0),
    ("disccraft-pgs", 0),
    ("disccraft-udf", 0),
    ("disccraft-photo", 0),
    ("disccraft-motion", 0),
    ("disccraft-sound", 0),
    ("filmcraft-time", 0),
    ("filmcraft-geom", 0),
    ("filmcraft-color", 0),
    ("filmcraft-bitstream", 0),
    ("filmcraft-mpeg2v", 0),
    ("filmcraft-mpegts", 0),
    ("filmcraft-ac3", 0),
    ("filmcraft-h264", 0),
    ("filmcraft-h264enc", 0),
    ("disccraft-project", 1),
    ("disccraft-vm", 1),
    ("filmcraft-frame", 1),
    ("filmcraft-media", 1),
    ("filmcraft-text", 1),
    ("filmcraft-audio-dsp", 1),
    ("disccraft-menu", 2),
    ("disccraft-nav", 2),
    ("disccraft-transcode", 2),
    ("disccraft-check", 2),
    ("disccraft-dvd", 2),
    ("disccraft-bd", 2),
    ("filmcraft-codecs", 2),
    ("disccraft-build", 3),
    ("disccraft-engine", 4),
    ("disccraft-ui-egui", 5),
    ("disccraft-automation", 5),
    ("disccraft-platform", 5),
    ("disccraft", 6),
    ("disccraft-cli", 6),
];

const GUI_CRATES: &[&str] = &[
    "egui",
    "eframe",
    "egui_extras",
    "egui-wgpu",
    "egui-winit",
    "egui_glow",
    "winit",
    "wgpu",
    "rfd",
];

const DENY_LINTS: &[&str] = &[
    "clippy::unwrap_used",
    "clippy::expect_used",
    "clippy::panic",
    "clippy::unimplemented",
    "clippy::todo",
    "clippy::unreachable",
];

pub fn run(root: &Path) -> Result<()> {
    let mut problems = Vec::new();
    let mut count = 0usize;

    for dir_name in ["crates", "apps"] {
        let base = root.join(dir_name);
        match collect_crate_dirs(&base) {
            Ok(dirs) => {
                for crate_dir in dirs {
                    count += 1;
                    problems.extend(check_crate_dir(&crate_dir));
                }
            }
            Err(problem) => problems.push(problem),
        }
    }

    if problems.is_empty() {
        println!("layers: {count} crates OK");
        Ok(())
    } else {
        for problem in &problems {
            println!("{problem}");
        }
        Err(format!("layers: {} problems", problems.len()))
    }
}

fn collect_crate_dirs(base: &Path) -> Result<Vec<PathBuf>> {
    if !base.is_dir() {
        return Ok(Vec::new());
    }

    let mut dirs = Vec::new();
    let entries = std::fs::read_dir(base)
        .map_err(|error| format!("failed to read {}: {error}", base.display()))?;

    for entry in entries {
        let entry = entry.map_err(|error| format!("failed to read {}: {error}", base.display()))?;
        let path = entry.path();
        if path.is_dir() && path.join("Cargo.toml").is_file() {
            dirs.push(path);
        }
    }

    dirs.sort();
    Ok(dirs)
}

fn check_crate_dir(crate_dir: &Path) -> Vec<String> {
    let manifest_path = crate_dir.join("Cargo.toml");
    let manifest_text = match std::fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) => {
            return vec![format!(
                "failed to read {}: {error}",
                manifest_path.display()
            )];
        }
    };

    let manifest: toml::Table = match manifest_text.parse() {
        Ok(manifest) => manifest,
        Err(error) => {
            return vec![format!(
                "failed to parse {}: {error}",
                manifest_path.display()
            )];
        }
    };

    let name_hint = crate_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");

    let crate_root_src = read_crate_root(crate_dir);
    check_manifest(name_hint, &manifest, crate_root_src.as_deref())
}

fn read_crate_root(crate_dir: &Path) -> Option<String> {
    let lib_path = crate_dir.join("src/lib.rs");
    if lib_path.is_file() {
        return std::fs::read_to_string(lib_path).ok();
    }

    let main_path = crate_dir.join("src/main.rs");
    if main_path.is_file() {
        return std::fs::read_to_string(main_path).ok();
    }

    None
}

fn check_manifest(
    name_hint: &str,
    manifest: &toml::Table,
    crate_root_src: Option<&str>,
) -> Vec<String> {
    let name = package_name(name_hint, manifest);
    let crate_layer = layer_of(&name);

    let mut problems = Vec::new();
    if crate_layer.is_none() {
        problems.push(format!(
            "unknown crate {name}: add it to xtask/src/layers.rs"
        ));
    }

    let dependencies = collect_dependencies(manifest);
    problems.extend(check_dependencies(&name, crate_layer, &dependencies));
    problems.extend(check_lints(&name, manifest));
    problems.extend(check_crate_root(&name, crate_root_src));
    problems
}

fn package_name<'a>(name_hint: &'a str, manifest: &'a toml::Table) -> String {
    let name = manifest
        .get("package")
        .and_then(|value| value.as_table())
        .and_then(|table| table.get("name"))
        .and_then(|value| value.as_str());

    match name {
        Some(name) => name.to_string(),
        None => name_hint.to_string(),
    }
}

fn layer_of(name: &str) -> Option<u8> {
    LAYERS
        .iter()
        .find(|(layer_name, _)| *layer_name == name)
        .map(|(_, layer)| *layer)
}

fn collect_dependencies(manifest: &toml::Table) -> Vec<String> {
    let mut dependencies = Vec::new();

    if let Some(table) = get_table(manifest, "dependencies") {
        dependencies.extend(dependency_names(table));
    }

    if let Some(table) = get_table(manifest, "build-dependencies") {
        dependencies.extend(dependency_names(table));
    }

    if let Some(target) = get_table(manifest, "target") {
        for (_cfg, cfg_value) in target.iter() {
            if let Some(cfg_table) = cfg_value.as_table() {
                if let Some(table) = get_table(cfg_table, "dependencies") {
                    dependencies.extend(dependency_names(table));
                }
                if let Some(table) = get_table(cfg_table, "build-dependencies") {
                    dependencies.extend(dependency_names(table));
                }
            }
        }
    }

    dependencies.sort();
    dependencies.dedup();
    dependencies
}

fn get_table<'a>(table: &'a toml::Table, key: &str) -> Option<&'a toml::Table> {
    table.get(key).and_then(|value| value.as_table())
}

fn dependency_names(table: &toml::Table) -> Vec<String> {
    let mut names = Vec::new();
    for (key, value) in table.iter() {
        if let Some(name) = dependency_name(key, value) {
            names.push(name);
        }
    }
    names
}

fn dependency_name(key: &str, value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(_) => Some(key.to_string()),
        toml::Value::Table(table) => {
            if let Some(package) = table.get("package").and_then(|package| package.as_str()) {
                return Some(package.to_string());
            }
            Some(key.to_string())
        }
        _ => Some(key.to_string()),
    }
}

fn check_dependencies(name: &str, crate_layer: Option<u8>, dependencies: &[String]) -> Vec<String> {
    let mut problems = Vec::new();

    for dependency in dependencies {
        if is_internal(dependency) {
            match layer_of(dependency) {
                Some(dependency_layer) => {
                    if let Some(crate_layer) = crate_layer
                        && dependency_layer > crate_layer
                    {
                        problems.push(format!(
                                "{name} (L{crate_layer}) must not depend on {dependency} (L{dependency_layer})"
                            ));
                    }
                }
                None => {
                    problems.push(format!(
                        "{name} depends on unknown crate {dependency}: add it to xtask/src/layers.rs"
                    ));
                }
            }
        }

        if let Some(crate_layer) = crate_layer
            && crate_layer < 5
            && is_gui(dependency)
        {
            problems.push(format!(
                "{name} (L{crate_layer}) must not depend on GUI crate {dependency}"
            ));
        }
    }

    problems
}

fn is_internal(name: &str) -> bool {
    name.starts_with("disccraft") || name.starts_with("filmcraft-")
}

fn is_gui(name: &str) -> bool {
    GUI_CRATES.contains(&name)
}

fn check_lints(name: &str, manifest: &toml::Table) -> Vec<String> {
    if name == "disccraft-platform" {
        return Vec::new();
    }

    let ok = get_table(manifest, "lints")
        .and_then(|table| table.get("workspace"))
        .and_then(|value| value.as_bool())
        == Some(true);

    if ok {
        Vec::new()
    } else {
        vec![format!("{name} must have [lints] workspace = true")]
    }
}

fn check_crate_root(name: &str, src: Option<&str>) -> Vec<String> {
    let Some(src) = src else {
        return vec![format!(
            "{name} missing crate root (src/lib.rs or src/main.rs)"
        )];
    };

    let mut problems = Vec::new();
    if !src.contains("#![deny(") {
        problems.push(format!("{name} crate root must contain #![deny("));
    }

    for lint in DENY_LINTS {
        if !src.contains(lint) {
            problems.push(format!("{name} crate root must contain {lint}"));
        }
    }

    problems
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::check_manifest;

    const VALID_ROOT: &str = "#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]\n";

    fn manifest(text: &str) -> toml::Table {
        text.parse::<toml::Table>().unwrap()
    }

    #[test]
    fn l4_depends_on_l1_passes() {
        let manifest = manifest(
            "[package]\nname = \"disccraft-engine\"\n[dependencies]\ndisccraft-project = { workspace = true }\n[lints]\nworkspace = true\n",
        );
        let problems = check_manifest("disccraft-engine", &manifest, Some(VALID_ROOT));
        assert!(problems.is_empty());
    }

    #[test]
    fn l1_depends_on_l4_fails() {
        let manifest = manifest(
            "[package]\nname = \"disccraft-project\"\n[dependencies]\ndisccraft-engine = { workspace = true }\n[lints]\nworkspace = true\n",
        );
        let problems = check_manifest("disccraft-project", &manifest, Some(VALID_ROOT));
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("must not depend on disccraft-engine (L4)"))
        );
    }

    #[test]
    fn l2_depends_on_gui_fails() {
        let manifest = manifest(
            "[package]\nname = \"disccraft-menu\"\n[dependencies]\negui = \"0.30\"\n[lints]\nworkspace = true\n",
        );
        let problems = check_manifest("disccraft-menu", &manifest, Some(VALID_ROOT));
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("must not depend on GUI crate egui"))
        );
    }

    #[test]
    fn l5_depends_on_gui_passes() {
        let manifest = manifest(
            "[package]\nname = \"disccraft-ui-egui\"\n[dependencies]\negui = \"0.30\"\n[lints]\nworkspace = true\n",
        );
        let problems = check_manifest("disccraft-ui-egui", &manifest, Some(VALID_ROOT));
        assert!(problems.is_empty());
    }

    #[test]
    fn unknown_crate_fails() {
        let manifest = manifest("[package]\nname = \"mystery\"\n[lints]\nworkspace = true\n");
        let problems = check_manifest("mystery", &manifest, Some(VALID_ROOT));
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("unknown crate mystery"))
        );
    }

    #[test]
    fn package_rename_is_resolved() {
        let manifest = manifest(
            "[package]\nname = \"disccraft-engine\"\n[dependencies]\nproject = { package = \"disccraft-project\", workspace = true }\n[lints]\nworkspace = true\n",
        );
        let problems = check_manifest("disccraft-engine", &manifest, Some(VALID_ROOT));
        assert!(problems.is_empty());
    }

    #[test]
    fn target_specific_dependencies_are_checked() {
        let manifest = manifest(
            "[package]\nname = \"disccraft-project\"\n[lints]\nworkspace = true\n[target.'cfg(unix)'.dependencies]\ndisccraft-engine = \"0.1\"\n",
        );
        let problems = check_manifest("disccraft-project", &manifest, Some(VALID_ROOT));
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("must not depend on disccraft-engine (L4)"))
        );
    }

    #[test]
    fn missing_lints_fails() {
        let manifest = manifest("[package]\nname = \"disccraft-engine\"\n");
        let problems = check_manifest("disccraft-engine", &manifest, Some(VALID_ROOT));
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("must have [lints] workspace = true"))
        );
    }

    #[test]
    fn platform_may_omit_lints() {
        let manifest = manifest("[package]\nname = \"disccraft-platform\"\n");
        let problems = check_manifest("disccraft-platform", &manifest, Some(VALID_ROOT));
        assert!(problems.is_empty());
    }

    #[test]
    fn crate_root_missing_todo_fails() {
        let root = "#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::unreachable)]\n";
        let manifest =
            manifest("[package]\nname = \"disccraft-engine\"\n[lints]\nworkspace = true\n");
        let problems = check_manifest("disccraft-engine", &manifest, Some(root));
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("clippy::todo"))
        );
    }
}
