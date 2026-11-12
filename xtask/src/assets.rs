use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, String>;

const ATTRIBUTION_SUFFIX: &str = ".attribution";

const REQUIRED_KEYS: &[&str] = &["author", "source", "license"];

const ALLOWED_LICENSES: &[&str] = &[
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "OFL-1.1",
    "CC0-1.0",
    "CC-BY-4.0",
    "CC-BY-SA-4.0",
    "MPL-2.0",
    "Unlicense",
    "LicenseRef-PublicDomain",
];

pub fn run(root: &Path) -> Result<()> {
    let assets_dir = root.join("assets");
    if !assets_dir.is_dir() {
        println!("assets: no assets/ directory");
        return Ok(());
    }

    let (count, problems) = check_assets(root)?;
    if problems.is_empty() {
        println!("assets: {count} files OK");
        Ok(())
    } else {
        for problem in &problems {
            println!("{problem}");
        }
        Err(format!("assets: {} problems", problems.len()))
    }
}

fn check_assets(root: &Path) -> Result<(usize, Vec<String>)> {
    let assets_dir = root.join("assets");
    let files = walk_files(&assets_dir)?;
    let (asset_paths, attribution_paths) = classify_files(&files);

    let mut problems = Vec::new();
    problems.extend(check_missing_attributions(root, &asset_paths));
    problems.extend(check_attribution_files(root, &attribution_paths));
    problems.extend(check_attribution_md(root, &asset_paths));

    Ok((asset_paths.len(), problems))
}

fn walk_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries = read_dir_sorted(dir)?;

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk_files(&path)?);
        } else if path.is_file() {
            files.push(path);
        }
    }

    Ok(files)
}

fn read_dir_sorted(dir: &Path) -> Result<Vec<std::fs::DirEntry>> {
    let mut entries = Vec::new();
    let read = std::fs::read_dir(dir)
        .map_err(|error| format!("failed to read {}: {error}", dir.display()))?;

    for entry in read {
        let entry = entry.map_err(|error| format!("failed to read {}: {error}", dir.display()))?;
        entries.push(entry);
    }

    entries.sort_by_key(|entry| entry.path());
    Ok(entries)
}

fn classify_files(files: &[PathBuf]) -> (Vec<PathBuf>, Vec<(PathBuf, String)>) {
    let mut assets = Vec::new();
    let mut attributions = Vec::new();

    for file in files {
        let Some(file_name) = file.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if file_name == ".gitkeep" {
            continue;
        }

        if let Some(stem) = file_name.strip_suffix(ATTRIBUTION_SUFFIX) {
            attributions.push((file.clone(), stem.to_string()));
        } else {
            assets.push(file.clone());
        }
    }

    (assets, attributions)
}

fn check_missing_attributions(root: &Path, asset_paths: &[PathBuf]) -> Vec<String> {
    let mut problems = Vec::new();

    for asset in asset_paths {
        let attribution = attribution_path_for(asset);
        if !attribution.is_file() {
            problems.push(format!(
                "{}: missing attribution file",
                relative_path(root, asset)
            ));
        }
    }

    problems
}

fn attribution_path_for(asset: &Path) -> PathBuf {
    let mut path = asset.as_os_str().to_os_string();
    path.push(ATTRIBUTION_SUFFIX);
    PathBuf::from(path)
}

fn check_attribution_files(root: &Path, attribution_paths: &[(PathBuf, String)]) -> Vec<String> {
    let mut problems = Vec::new();

    for (attribution_path, stem) in attribution_paths {
        problems.extend(check_attribution_file(root, attribution_path, stem));
    }

    problems
}

fn check_attribution_file(root: &Path, attribution_path: &Path, stem: &str) -> Vec<String> {
    let display = relative_path(root, attribution_path);
    let mut problems = Vec::new();

    if stem.is_empty() {
        problems.push(format!("{display}: invalid attribution file name"));
        return problems;
    }

    match attribution_path.parent().map(|parent| parent.join(stem)) {
        Some(asset_path) if asset_path.is_file() => {}
        Some(_) => problems.push(format!("{display}: orphan attribution file")),
        None => problems.push(format!("{display}: cannot resolve asset path")),
    }

    match std::fs::read_to_string(attribution_path) {
        Ok(content) => problems.extend(attribution_problems(&display, &content)),
        Err(error) => problems.push(format!("failed to read {display}: {error}")),
    }

    problems
}

fn check_attribution_md(root: &Path, asset_paths: &[PathBuf]) -> Vec<String> {
    if asset_paths.is_empty() {
        return Vec::new();
    }

    let attribution_md = root.join("ATTRIBUTION.md");
    match std::fs::read_to_string(attribution_md) {
        Ok(markdown) => {
            let mut problems = Vec::new();
            for asset in asset_paths {
                let relative = relative_path(root, asset);
                if !markdown.contains(&relative) {
                    problems.push(format!("ATTRIBUTION.md missing entry for {relative}"));
                }
            }
            problems
        }
        Err(_) => vec!["ATTRIBUTION.md is missing".to_string()],
    }
}

fn relative_path(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) => relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => path.display().to_string(),
    }
}

fn attribution_problems(display: &str, content: &str) -> Vec<String> {
    let mut fields = BTreeMap::new();
    let mut problems = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match line.split_once(':') {
            Some((key, value)) => {
                fields.insert(key.trim().to_string(), value.trim().to_string());
            }
            None => problems.push(format!("{display}: invalid attribution line: {line}")),
        }
    }

    for key in REQUIRED_KEYS {
        match fields.get(*key) {
            Some(value) if !value.is_empty() => {}
            Some(_) => problems.push(format!("{display}: {key} must be non-empty")),
            None => problems.push(format!("{display}: missing {key}")),
        }
    }

    if let Some(license) = fields.get("license")
        && !license.is_empty()
    {
        problems.extend(license_problems(display, license));
    }

    problems
}

fn license_problems(display: &str, expression: &str) -> Vec<String> {
    let mut problems = Vec::new();

    for part in split_license(expression) {
        if part.is_empty() {
            problems.push(format!("{display}: empty license part"));
        } else if !ALLOWED_LICENSES.contains(&part.as_str()) {
            problems.push(format!("{display}: unsupported license {part}"));
        }
    }

    problems
}

fn split_license(expression: &str) -> Vec<String> {
    expression
        .replace(" AND ", " OR ")
        .split(" OR ")
        .map(|part| part.trim().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{Result, attribution_problems, check_assets, license_problems};
    use std::path::{Path, PathBuf};

    #[test]
    fn valid_attribution() {
        let content = "author: Someone\nsource: https://example.invalid\nlicense: MIT\n";
        let problems = attribution_problems("assets/play.svg.attribution", content);
        assert!(problems.is_empty());
    }

    #[test]
    fn missing_key() {
        let content = "author: Someone\nsource: https://example.invalid\n";
        let problems = attribution_problems("assets/play.svg.attribution", content);
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("missing license"))
        );
    }

    #[test]
    fn bad_license() {
        let content = "author: Someone\nsource: https://example.invalid\nlicense: GPL-3.0\n";
        let problems = attribution_problems("assets/play.svg.attribution", content);
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("unsupported license GPL-3.0"))
        );
    }

    #[test]
    fn allowed_or_license() {
        let problems = license_problems("assets/play.svg.attribution", "MIT OR Apache-2.0");
        assert!(problems.is_empty());
    }

    #[test]
    fn filesystem_missing_attribution_and_md() -> Result<()> {
        let root = temp_root("missing-attribution");
        let _ = std::fs::remove_dir_all(&root);

        let result = filesystem_case(&root);
        let cleanup = std::fs::remove_dir_all(&root);

        result?;
        cleanup.map_err(|error| format!("cleanup failed: {error}"))
    }

    fn temp_root(suffix: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "disccraft-xtask-assets-{}-{suffix}",
            std::process::id()
        ));
        path
    }

    fn filesystem_case(root: &Path) -> Result<()> {
        let assets = root.join("assets");
        let icons = assets.join("icons");

        std::fs::create_dir_all(&icons)
            .map_err(|error| format!("failed to create {}: {error}", icons.display()))?;
        std::fs::write(icons.join("play.svg"), b"<svg/>")
            .map_err(|error| format!("failed to write play.svg: {error}"))?;

        let (count, problems) = check_assets(root)?;
        if count != 1 {
            return Err(format!("expected 1 asset, got {count}"));
        }

        if !problems
            .iter()
            .any(|problem| problem.contains("missing attribution file"))
        {
            return Err("missing attribution problem was not reported".to_string());
        }

        if !problems
            .iter()
            .any(|problem| problem.contains("ATTRIBUTION.md is missing"))
        {
            return Err("missing ATTRIBUTION.md problem was not reported".to_string());
        }

        Ok(())
    }
}
